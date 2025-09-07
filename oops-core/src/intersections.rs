//! Functions for detecting and analyzing OSM intersections.
//!
//! This module includes logic for finding topological intersections (shared nodes),
//! geometric intersections (unsplit ways), and clustering them into logical groups.

use crate::data::{OsmData, OsmWay};
use crate::graph::RoadGraph;
use crate::spatial::{way_to_linestring, WayWithLineString};
use geo::{Contains, HaversineDistance, Intersects, Point, Rect};
use petgraph::graph::NodeIndex;
use petgraph::Direction;
use rstar::{RTree, RTreeObject, AABB};
use std::collections::{HashMap, HashSet};

/// Represents a logical intersection as a cluster of related intersection nodes.
#[derive(Debug, Clone)]
pub struct LogicalIntersection {
    pub representative_node: i64,
    pub node_cluster: Vec<i64>,
}

/// Wrapper for intersection nodes to use with RTree.
#[derive(Debug, Clone)]
pub struct IntersectionNodePoint {
    pub node_id: i64,
    pub point: Point,
    pub degree: usize,
}

impl RTreeObject for IntersectionNodePoint {
    type Envelope = AABB<Point>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(self.point, self.point)
    }
}

/// Check if a way represents a road that can have intersections.
pub fn is_road_way(way: &OsmWay) -> bool {
    way.tags.get("highway").is_some_and(|h| {
        matches!(
            h.as_str(),
            "primary" | "secondary" | "tertiary" | "unclassified" | "residential" | "trunk" | "motorway" |
            "primary_link" | "secondary_link" | "tertiary_link" | "trunk_link" | "motorway_link"
        )
    })
}

/// Find all intersections in a road network (degree 3+).
pub fn find_road_intersections(data: &OsmData) -> Vec<i64> {
    let mut node_way_count: HashMap<i64, usize> = HashMap::new();
    for way in data.ways.values() {
        if is_road_way(way) {
            for &node_id in &way.nodes {
                *node_way_count.entry(node_id).or_insert(0) += 1;
            }
        }
    }
    node_way_count
        .into_iter()
        .filter_map(|(node_id, count)| if count >= 3 { Some(node_id) } else { None })
        .collect()
}

/// Find intersections within a geographic bounding box.
pub fn find_road_intersections_in_bbox(data: &OsmData, bbox: Rect) -> Vec<i64> {
    let mut node_way_count: HashMap<i64, usize> = HashMap::new();
    for way in data.ways.values() {
        if is_road_way(way) {
            for &node_id in &way.nodes {
                if let Some(node) = data.nodes.get(&node_id) {
                    if bbox.contains(&Point::new(node.lon, node.lat)) {
                        *node_way_count.entry(node_id).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    node_way_count
        .into_iter()
        .filter_map(|(node_id, count)| if count >= 3 { Some(node_id) } else { None })
        .collect()
}

/// Find logical intersections by clustering nearby intersection nodes (RTree optimized).
pub fn find_logical_intersections(
    data: &OsmData,
    max_cluster_distance: f64,
) -> Vec<LogicalIntersection> {
    let intersection_nodes = find_road_intersections(data);
    if intersection_nodes.is_empty() {
        return Vec::new();
    }

    let node_points: Vec<IntersectionNodePoint> = intersection_nodes
        .iter()
        .filter_map(|&node_id| {
            data.nodes.get(&node_id).map(|node| IntersectionNodePoint {
                node_id,
                point: Point::new(node.lon, node.lat),
                degree: 0,
            })
        })
        .collect();

    let rtree = RTree::bulk_load(node_points);
    let mut clusters: Vec<Vec<i64>> = Vec::new();
    let mut used_nodes = HashSet::new();

    for &node_id in &intersection_nodes {
        if used_nodes.contains(&node_id) {
            continue;
        }

        let mut cluster = vec![node_id];
        used_nodes.insert(node_id);
        let mut queue = vec![node_id];

        while let Some(current_node_id) = queue.pop() {
            if let Some(node) = data.nodes.get(&current_node_id) {
                let query_point = Point::new(node.lon, node.lat);
                let buffer_degrees = max_cluster_distance / 111_320.0;
                let search_envelope = AABB::from_corners(
                    (query_point.x() - buffer_degrees, query_point.y() - buffer_degrees).into(),
                    (query_point.x() + buffer_degrees, query_point.y() + buffer_degrees).into(),
                );

                for candidate in rtree.locate_in_envelope_intersecting(&search_envelope) {
                    if !used_nodes.contains(&candidate.node_id)
                        && query_point.haversine_distance(&candidate.point) <= max_cluster_distance
                    {
                        used_nodes.insert(candidate.node_id);
                        queue.push(candidate.node_id);
                        cluster.push(candidate.node_id);
                    }
                }
            }
        }
        clusters.push(cluster);
    }

    clusters
        .into_iter()
        .map(|cluster| LogicalIntersection {
            representative_node: cluster[0],
            node_cluster: cluster,
        })
        .collect()
}

/// Represents a geometric intersection point where ways cross but aren't split.
#[derive(Debug, Clone)]
pub struct GeometricIntersection {
    pub point: geo::Point,
    pub intersecting_ways: Vec<i64>,
    pub degree: usize,
}

/// Find geometric intersections where ways cross but nodes aren't split.
pub fn find_geometric_intersections(
    data: &OsmData,
    bbox: Option<Rect>,
) -> Vec<GeometricIntersection> {
    let road_ways: Vec<WayWithLineString> = data.ways.values()
        .filter(|way| is_road_way(way))
        .filter_map(|way| way_to_linestring(way, data).map(|l| WayWithLineString { way: way.clone(), linestring: l }))
        .filter(|wwl| bbox.map_or(true, |b| wwl.linestring.intersects(&b)))
        .collect();

    let rtree = RTree::bulk_load(road_ways);
    let mut intersections = Vec::new();
    let mut processed_pairs = HashSet::new();

    for wwl1 in rtree.iter() {
        let search_envelope = wwl1.linestring.envelope();
        for wwl2 in rtree.locate_in_envelope_intersecting(&search_envelope) {
            let pair = if wwl1.way.id < wwl2.way.id { (wwl1.way.id, wwl2.way.id) } else { (wwl2.way.id, wwl1.way.id) };
            if processed_pairs.contains(&pair) { continue; }
            processed_pairs.insert(pair);

            if wwl1.way.nodes.iter().any(|n| wwl2.way.nodes.contains(n)) { continue; }

            if wwl1.linestring.intersects(&wwl2.linestring) {
                // A simple approximation for intersection point
                if let Some(p1) = wwl1.linestring.points().next() {
                     intersections.push(GeometricIntersection {
                        point: p1,
                        intersecting_ways: vec![wwl1.way.id, wwl2.way.id],
                        degree: 2,
                    });
                }
            }
        }
    }
    intersections
}

/// Extract comprehensive metadata for a logical intersection.
pub fn extract_intersection_metadata(
    logical_intersection: &LogicalIntersection,
    data: &OsmData,
    node_to_ways: &HashMap<i64, Vec<i64>>,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    metadata.insert("representative_node".into(), logical_intersection.representative_node.to_string());
    metadata.insert("cluster_size".into(), logical_intersection.node_cluster.len().to_string());
    metadata.insert("node_cluster".into(), logical_intersection.node_cluster.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","));

    if let Some(node) = data.nodes.get(&logical_intersection.representative_node) {
        metadata.insert("coordinates".into(), format!("{},{}", node.lat, node.lon));
    }

    let mut connected_ways = HashSet::new();
    for &node_id in &logical_intersection.node_cluster {
        if let Some(way_ids) = node_to_ways.get(&node_id) {
            for &way_id in way_ids {
                connected_ways.insert(way_id);
            }
        }
    }

    let ways: Vec<_> = connected_ways.iter().filter_map(|id| data.ways.get(id)).collect();
    metadata.insert("degree".into(), ways.len().to_string());
    metadata.insert("connected_ways".into(), ways.iter().map(|w| w.id.to_string()).collect::<Vec<_>>().join(","));

    let highway_types: Vec<_> = ways.iter().filter_map(|w| w.tags.get("highway").cloned()).collect();
    metadata.insert("highway_types".into(), highway_types.join(","));

    // ... other metadata extraction
    metadata
}

/// Find complex intersections (roundabouts, multi-way junctions) using graph-based analysis.
pub fn find_complex_intersections_graph(
    data: &OsmData,
    graph: &RoadGraph,
    node_map: &HashMap<i64, NodeIndex>,
) -> Vec<i64> {
    let mut complex_intersections = Vec::new();
    for (&osm_node_id, &graph_idx) in node_map {
        let mut connected_ways = HashSet::new();
        for edge_ref in graph.edges_directed(graph_idx, Direction::Outgoing) {
            connected_ways.insert(edge_ref.weight().osm_way_id);
        }
        for edge_ref in graph.edges_directed(graph_idx, Direction::Incoming) {
            connected_ways.insert(edge_ref.weight().osm_way_id);
        }
        let degree = connected_ways.len();

        let has_roundabout = connected_ways.iter().any(|way_id| {
            data.ways.get(way_id).map_or(false, |w| w.tags.get("junction") == Some(&"roundabout".to_string()))
        });

        if degree >= 4 || has_roundabout {
            complex_intersections.push(osm_node_id);
        }
    }
    complex_intersections
}

