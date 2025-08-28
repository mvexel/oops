//! OSM-specific analysis utilities
//!
//! Functions for analyzing OpenStreetMap data structures and relationships

use crate::{OsmData, OsmWay};
use geo::*;
use std::collections::HashMap;

/// Check if a way represents a road that can have intersections
///
/// Filters out footways, cycleways, and other non-road infrastructure
///
/// # Arguments
/// * `way` - OSM way to check
///
/// # Returns
/// True if the way is a road that should be considered for intersections
pub fn is_road_way(way: &OsmWay) -> bool {
    way.tags.get("highway").map_or(false, |highway| {
        matches!(
            highway.as_str(),
            "primary"
                | "secondary"
                | "tertiary"
                | "unclassified"
                | "residential"
                | "trunk"
                | "motorway"
                | "primary_link"
                | "secondary_link"
                | "tertiary_link"
                | "trunk_link"
                | "motorway_link"
        )
    })
}

/// Find all intersections in a road network
///
/// An intersection is defined as a node that is shared by 3 or more ways
/// (excluding 2-way intersections which are just road segments)
///
/// # Arguments
/// * `data` - OSM data containing ways and nodes
///
/// # Returns
/// Vector of node IDs that represent intersections (degree 3+)
pub fn find_road_intersections(data: &OsmData) -> Vec<i64> {
    let mut node_way_count: HashMap<i64, usize> = HashMap::new();

    // Count how many ways each node belongs to
    for way in data.ways.values() {
        if is_road_way(way) {
            for &node_id in &way.nodes {
                *node_way_count.entry(node_id).or_insert(0) += 1;
            }
        }
    }

    // Find nodes with degree 3 or higher (true intersections)
    node_way_count
        .into_iter()
        .filter_map(
            |(node_id, count)| {
                if count >= 3 {
                    Some(node_id)
                } else {
                    None
                }
            },
        )
        .collect()
}

/// Find intersections within a geographic bounding box
///
/// # Arguments
/// * `data` - OSM data containing ways and nodes
/// * `bbox` - Bounding rectangle to filter intersections
///
/// # Returns
/// Vector of node IDs for intersections within the bbox
pub fn find_road_intersections_in_bbox(data: &OsmData, bbox: Rect) -> Vec<i64> {
    find_road_intersections(data)
        .into_iter()
        .filter(|&node_id| {
            if let Some(node) = data.nodes.get(&node_id) {
                let point = Point::new(node.lon, node.lat);
                bbox.contains(&point)
            } else {
                false
            }
        })
        .collect()
}

/// Find complex intersections (roundabouts, multi-way junctions)
///
/// These require special handling for pedestrian crossing analysis
///
/// # Arguments
/// * `data` - OSM data
/// * `intersection_nodes` - List of intersection node IDs to analyze
///
/// # Returns
/// Vector of complex intersection node IDs (degree 4+, roundabouts)
pub fn find_complex_intersections(data: &OsmData, intersection_nodes: &[i64]) -> Vec<i64> {
    let mut complex_intersections = Vec::new();

    for &node_id in intersection_nodes {
        let connected_ways: Vec<_> = data
            .ways
            .values()
            .filter(|way| is_road_way(way) && way.nodes.contains(&node_id))
            .collect();

        // Complex if 4+ ways or contains roundabout
        if connected_ways.len() >= 4 {
            complex_intersections.push(node_id);
        } else if connected_ways.iter().any(|way| {
            way.tags
                .get("junction")
                .map_or(false, |j| j == "roundabout")
        }) {
            complex_intersections.push(node_id);
        }
    }

    complex_intersections
}

/// Convert an OSM way to a LineString geometry
///
/// # Arguments
/// * `way` - OSM way to convert
/// * `data` - OSM data containing node coordinates
///
/// # Returns
/// LineString if all nodes are found, None otherwise
pub fn way_to_linestring(way: &OsmWay, data: &OsmData) -> Option<LineString> {
    let coords: Result<Vec<Coord>, _> = way
        .nodes
        .iter()
        .map(|&node_id| {
            data.nodes
                .get(&node_id)
                .map(|node| Coord {
                    x: node.lon,
                    y: node.lat,
                })
                .ok_or(())
        })
        .collect();

    coords.ok().map(LineString::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_data() -> OsmData {
        let mut data = OsmData::new();

        // Create a simple T-intersection (lon, lat order)
        data.add_node(crate::OsmNode::new(1, 0.0, 0.0));
        data.add_node(crate::OsmNode::new(2, 0.001, 0.0));
        data.add_node(crate::OsmNode::new(3, -0.001, 0.0));
        data.add_node(crate::OsmNode::new(4, 0.0, 0.001));

        // Way 1: North-South road
        let mut way1 = crate::OsmWay::new(1, vec![2, 1, 3]);
        way1.tags.insert("highway".into(), "residential".into());
        data.ways.insert(1, way1);

        // Way 2: East-West road
        let mut way2 = crate::OsmWay::new(2, vec![1, 4]);
        way2.tags.insert("highway".into(), "residential".into());
        data.ways.insert(2, way2);

        data
    }

    #[test]
    fn test_is_road_way() {
        let way = crate::OsmWay::new(1, vec![1, 2]);
        let mut way_with_tags = way.clone();
        way_with_tags
            .tags
            .insert("highway".into(), "residential".into());

        assert!(is_road_way(&way_with_tags));

        // Test non-road way
        let mut footway = crate::OsmWay::new(2, vec![1, 2]);
        footway.tags.insert("highway".into(), "footway".into());

        assert!(!is_road_way(&footway));
    }

    #[test]
    fn test_find_road_intersections() {
        let data = create_test_data();
        let intersections = find_road_intersections(&data);

        // Node 1 connects to 2 ways, so no intersection (need 3+)
        assert_eq!(intersections.len(), 0);
    }

    #[test]
    fn test_create_three_way_intersection() {
        let mut data = create_test_data();

        // Add a third way to create a true intersection
        data.add_node(crate::OsmNode::new(5, 0.0, -0.001));

        let mut way3 = crate::OsmWay::new(3, vec![1, 5]);
        way3.tags.insert("highway".into(), "residential".into());
        data.ways.insert(3, way3);

        let intersections = find_road_intersections(&data);

        // Now node 1 should be an intersection (degree 3)
        assert_eq!(intersections.len(), 1);
        assert_eq!(intersections[0], 1);
    }

    #[test]
    fn test_way_to_linestring() {
        let data = create_test_data();
        let way = data.ways.get(&1).unwrap();

        let linestring = way_to_linestring(way, &data).unwrap();

        assert_eq!(linestring.coords().count(), 3);

        let coords: Vec<_> = linestring.coords().collect();
        // Way 1 goes [node2, node1, node3] with (lon, lat):
        // Node 2: (0.001, 0.0), Node 1: (0.0, 0.0), Node 3: (-0.001, 0.0)
        assert_eq!(coords[0], &Coord { x: 0.001, y: 0.0 });
        assert_eq!(coords[1], &Coord { x: 0.0, y: 0.0 });
        assert_eq!(coords[2], &Coord { x: -0.001, y: 0.0 });
    }
}
