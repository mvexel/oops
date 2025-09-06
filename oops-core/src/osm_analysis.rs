//! OSM-specific analysis utilities
//!
//! Functions for analyzing OpenStreetMap data structures and relationships

use crate::{OsmData, OsmWay};
use geo::*;
use rstar::{RTree, RTreeObject, AABB};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WayWithLineString {
    pub way: OsmWay,
    pub linestring: LineString,
}

impl RTreeObject for WayWithLineString {
    type Envelope = AABB<Point>;

    fn envelope(&self) -> Self::Envelope {
        self.linestring.envelope()
    }
}

/// Finds ways from a given collection of candidates that are spatially close to a target way.
///
/// # Arguments
/// * `target_way` - The way for which to find adjacent candidates.
/// * `candidate_rtree` - An R-Tree containing all the candidate ways to search through.
/// * `max_distance` - The maximum distance in meters for a candidate to be considered adjacent.
/// * `data` - The main OSM data object for node lookups.
///
/// # Returns
/// A vector of candidate ways that are considered adjacent.
pub fn find_adjacent_way_candidates<'a>(
    target_way: &OsmWay,
    candidate_rtree: &'a RTree<WayWithLineString>,
    max_distance: f64,
    data: &OsmData,
) -> Vec<&'a WayWithLineString> {
    // Validate inputs
    if max_distance <= 0.0 || max_distance > 1000.0 {
        return Vec::new();
    }
    
    if let Some(target_linestring) = way_to_linestring(target_way, data) {
        let target_envelope = target_linestring.envelope();
        let buffer_distance = max_distance / 111_320.0; // Rough conversion from meters to degrees
        let bigger_envelope: AABB<Point> = AABB::from_corners(
            (target_envelope.lower().x() - buffer_distance, target_envelope.lower().y() - buffer_distance).into(),
            (target_envelope.upper().x() + buffer_distance, target_envelope.upper().y() + buffer_distance).into(),
        );
        candidate_rtree
            .locate_in_envelope_intersecting(&bigger_envelope)
            .filter(|candidate| {
                // Calculate minimum distance between linestrings
                let min_dist = candidate.linestring.points()
                    .map(|p1| {
                        target_linestring.points()
                            .map(|p2| p1.haversine_distance(&p2))
                            .fold(f64::MAX, f64::min)
                    })
                    .fold(f64::MAX, f64::min);

                min_dist.is_finite() && min_dist <= max_distance
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Checks if two ways have parallel segments by comparing bearings of individual segments
///
/// This handles cases where sidewalks wrap around corners but have parallel portions
///
/// # Arguments
/// * `way1` - First way to compare (typically the road)
/// * `way2` - Second way to compare (typically the sidewalk)
/// * `max_angle_diff` - Maximum angle difference in degrees to consider parallel
///
/// # Returns
/// True if any significant portions of the ways are parallel within the angle threshold
pub fn are_ways_parallel(way1: &LineString, way2: &LineString, max_angle_diff: f64) -> bool {
    let way1_points: Vec<_> = way1.points().collect();
    let way2_points: Vec<_> = way2.points().collect();
    
    // Validate input
    if way1_points.len() < 2 || way2_points.len() < 2 {
        return false;
    }
    
    if !(0.0..=180.0).contains(&max_angle_diff) {
        return false;
    }
    
    // For very short ways, fall back to overall bearing comparison
    if way1_points.len() <= 3 && way2_points.len() <= 3 {
        return are_ways_parallel_simple(way1, way2, max_angle_diff);
    }
    
    // Calculate segment bearings for both ways
    let way1_segments = calculate_segment_bearings(&way1_points);
    let way2_segments = calculate_segment_bearings(&way2_points);
    
    if way1_segments.is_empty() || way2_segments.is_empty() {
        return false;
    }
    
    // Check if any segments are parallel
    let mut parallel_score = 0.0;
    
    for (way1_bearing, way1_length) in &way1_segments {
        for (way2_bearing, way2_length) in &way2_segments {
            let angle_diff = (way1_bearing - way2_bearing).abs();
            let is_parallel = angle_diff <= max_angle_diff || (angle_diff - 180.0).abs() <= max_angle_diff;
            
            if is_parallel {
                // Weight by the length of the segments
                let weight = (way1_length + way2_length) / 2.0;
                parallel_score += weight;
            }
        }
    }
    
    // Calculate total length of way1 (the road we're analyzing)
    let total_way1_length: f64 = way1_segments.iter().map(|(_, length)| length).sum();
    
    // Consider ways parallel if parallel segments represent significant portion of the road
    let parallel_ratio = parallel_score / total_way1_length;
    parallel_ratio > 0.3 // At least 30% of segments should be parallel
}

/// Calculate bearing and length for each segment of a way
fn calculate_segment_bearings(points: &[geo::Point]) -> Vec<(f64, f64)> {
    let mut segments = Vec::new();
    
    for i in 0..points.len()-1 {
        let p1 = &points[i];
        let p2 = &points[i + 1];
        
        // Skip zero-length segments
        if p1 == p2 {
            continue;
        }
        
        let bearing = p1.haversine_bearing(*p2);
        let length = p1.haversine_distance(p2);
        segments.push((bearing, length));
    }
    
    segments
}

/// Simple bearing comparison for short ways (fallback method)
fn are_ways_parallel_simple(way1: &LineString, way2: &LineString, max_angle_diff: f64) -> bool {
    let way1_points: Vec<_> = way1.points().collect();
    let way2_points: Vec<_> = way2.points().collect();
    
    let way1_start = &way1_points[0];
    let way1_end = &way1_points[way1_points.len() - 1];
    let way2_start = &way2_points[0];
    let way2_end = &way2_points[way2_points.len() - 1];
    
    // Check for zero-length ways
    if way1_start == way1_end || way2_start == way2_end {
        return false;
    }
    
    let way1_bearing = way1_start.haversine_bearing(*way1_end);
    let way2_bearing = way2_start.haversine_bearing(*way2_end);
    
    let angle_diff = (way1_bearing - way2_bearing).abs();
    
    // Check if parallel (similar bearings) or anti-parallel (opposite bearings)
    angle_diff <= max_angle_diff || (angle_diff - 180.0).abs() <= max_angle_diff
}

/// Finds ways that are both spatially close and roughly parallel to a target way
///
/// # Arguments
/// * `target_way` - The way for which to find parallel candidates
/// * `candidate_rtree` - An R-Tree containing all the candidate ways to search through
/// * `max_distance` - The maximum distance in meters for a candidate to be considered adjacent
/// * `max_angle_diff` - The maximum angle difference in degrees to be considered parallel
/// * `data` - The main OSM data object for node lookups
///
/// # Returns
/// A vector of candidate ways that are considered adjacent and parallel
pub fn find_parallel_adjacent_ways<'a>(
    target_way: &OsmWay,
    candidate_rtree: &'a RTree<WayWithLineString>,
    max_distance: f64,
    max_angle_diff: f64,
    data: &OsmData,
) -> Vec<&'a WayWithLineString> {
    if let Some(target_linestring) = way_to_linestring(target_way, data) {
        find_adjacent_way_candidates(target_way, candidate_rtree, max_distance, data)
            .into_iter()
            .filter(|candidate| {
                are_ways_parallel(&target_linestring, &candidate.linestring, max_angle_diff)
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Calculates what portion of a target way is covered by nearby parallel sidewalks
///
/// # Arguments
/// * `target_way` - The road way to analyze
/// * `parallel_sidewalks` - List of nearby parallel sidewalks
/// * `max_distance` - Maximum distance to consider for coverage
/// * `data` - OSM data for geometry lookups
///
/// # Returns
/// Coverage ratio (0.0 to 1.0) indicating what portion of the road has nearby sidewalk coverage
pub fn calculate_sidewalk_coverage(
    target_way: &OsmWay,
    parallel_sidewalks: &[&WayWithLineString], 
    max_distance: f64,
    data: &OsmData,
) -> f64 {
    if let Some(target_linestring) = way_to_linestring(target_way, data) {
        let target_points: Vec<_> = target_linestring.points().collect();
        if target_points.len() < 2 {
            return 0.0;
        }
        
        // Sample points along the target road
        let sample_count = (target_points.len() * 3).max(10); // At least 10 samples, more for longer ways
        let mut covered_samples = 0;
        
        for i in 0..sample_count {
            // Interpolate point along the road
            let t = i as f64 / (sample_count - 1) as f64;
            let segment_index = ((target_points.len() - 1) as f64 * t) as usize;
            let segment_t = ((target_points.len() - 1) as f64 * t) - segment_index as f64;
            
            let sample_point = if segment_index + 1 < target_points.len() {
                let p1 = &target_points[segment_index];
                let p2 = &target_points[segment_index + 1];
                
                // Linear interpolation between points
                let lon = p1.x() + (p2.x() - p1.x()) * segment_t;
                let lat = p1.y() + (p2.y() - p1.y()) * segment_t;
                geo::Point::new(lon, lat)
            } else {
                target_points[segment_index].clone()
            };
            
            // Check if this sample point has nearby sidewalk coverage
            // Use point-to-line-segment distance for more accurate results
            let has_coverage = parallel_sidewalks.iter().any(|sidewalk| {
                let min_dist = calculate_point_to_linestring_distance(&sample_point, &sidewalk.linestring);
                min_dist <= max_distance
            });
            
            if has_coverage {
                covered_samples += 1;
            }
        }
        
        covered_samples as f64 / sample_count as f64
    } else {
        0.0
    }
}

/// Calculate the minimum distance from a point to a LineString using point-to-segment distance
///
/// This is more accurate than point-to-point distance as it considers the entire line segments,
/// not just the nodes.
///
/// # Arguments  
/// * `point` - The point to measure from
/// * `linestring` - The LineString to measure to
///
/// # Returns
/// Minimum distance in meters from the point to any segment of the linestring
pub fn calculate_point_to_linestring_distance(point: &geo::Point, linestring: &geo::LineString) -> f64 {
    let points: Vec<_> = linestring.points().collect();
    if points.len() < 2 {
        // Degenerate case - treat as point-to-point distance
        return points.get(0).map_or(f64::MAX, |p| point.haversine_distance(p));
    }
    
    let mut min_distance = f64::MAX;
    
    // Check distance to each line segment
    for i in 0..points.len() - 1 {
        let p1 = &points[i];
        let p2 = &points[i + 1];
        
        let segment_distance = calculate_point_to_segment_distance(point, p1, p2);
        min_distance = min_distance.min(segment_distance);
    }
    
    min_distance
}

/// Calculate the minimum distance from a point to a line segment
///
/// # Arguments
/// * `point` - The point to measure from  
/// * `seg_start` - Start point of the line segment
/// * `seg_end` - End point of the line segment
///
/// # Returns
/// Distance in meters from point to the closest point on the line segment
fn calculate_point_to_segment_distance(point: &geo::Point, seg_start: &geo::Point, seg_end: &geo::Point) -> f64 {
    // Handle degenerate case where segment is actually a point
    if seg_start == seg_end {
        return point.haversine_distance(seg_start);
    }
    
    // For short distances, we can use an approximate Cartesian approach
    // This is much faster and sufficiently accurate for distances < 1km
    let px = point.x();
    let py = point.y();
    let x1 = seg_start.x();
    let y1 = seg_start.y();
    let x2 = seg_end.x();
    let y2 = seg_end.y();
    
    // Vector from segment start to point
    let a = px - x1;
    let b = py - y1;
    
    // Vector from segment start to segment end
    let c = x2 - x1;
    let d = y2 - y1;
    
    let dot = a * c + b * d;
    let len_sq = c * c + d * d;
    
    if len_sq == 0.0 {
        return point.haversine_distance(seg_start);
    }
    
    let param = dot / len_sq;
    
    let closest_point = if param < 0.0 {
        // Closest point is the segment start
        *seg_start
    } else if param > 1.0 {
        // Closest point is the segment end  
        *seg_end
    } else {
        // Closest point is on the segment
        geo::Point::new(x1 + param * c, y1 + param * d)
    };
    
    point.haversine_distance(&closest_point)
}


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
    way.tags.get("highway").is_some_and(|highway| {
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
        if connected_ways.len() >= 4 || connected_ways.iter().any(|way| {
            way.tags
                .get("junction")
                .is_some_and(|j| j == "roundabout")
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

    #[test]
    fn test_are_ways_parallel() {
        // Create two parallel horizontal lines
        let way1 = LineString::from(vec![(0.0, 0.0), (1.0, 0.0)]);
        let way2 = LineString::from(vec![(0.0, 0.1), (1.0, 0.1)]);
        
        assert!(are_ways_parallel(&way1, &way2, 5.0));
        
        // Create perpendicular ways
        let way3 = LineString::from(vec![(0.0, 0.0), (0.0, 1.0)]);
        assert!(!are_ways_parallel(&way1, &way3, 5.0));
        
        // Test anti-parallel (opposite direction)
        let way4 = LineString::from(vec![(1.0, 0.1), (0.0, 0.1)]);
        assert!(are_ways_parallel(&way1, &way4, 5.0));
    }

    #[test]
    fn test_find_adjacent_way_candidates() {
        let mut data = OsmData::new();
        
        // Create test nodes
        data.add_node(crate::OsmNode::new(1, 0.0, 0.0));
        data.add_node(crate::OsmNode::new(2, 0.001, 0.0));
        data.add_node(crate::OsmNode::new(3, 0.0, 0.0001)); // ~11m north
        data.add_node(crate::OsmNode::new(4, 0.001, 0.0001));
        
        // Create a road
        let road = crate::OsmWay::new(1, vec![1, 2]);
        
        // Create a nearby sidewalk
        let mut sidewalk = crate::OsmWay::new(2, vec![3, 4]);
        sidewalk.tags.insert("highway".into(), "footway".into());
        sidewalk.tags.insert("footway".into(), "sidewalk".into());
        
        data.ways.insert(2, sidewalk.clone());
        
        // Build R-tree
        let sidewalks: Vec<_> = vec![WayWithLineString {
            way: sidewalk,
            linestring: way_to_linestring(&data.ways[&2], &data).unwrap(),
        }];
        let rtree = RTree::bulk_load(sidewalks);
        
        // Test finding adjacent candidates
        let candidates = find_adjacent_way_candidates(&road, &rtree, 20.0, &data);
        assert_eq!(candidates.len(), 1);
        
        // Test with smaller distance threshold
        let candidates = find_adjacent_way_candidates(&road, &rtree, 5.0, &data);
        assert_eq!(candidates.len(), 0);
    }

    #[test]
    fn test_find_parallel_adjacent_ways() {
        let mut data = OsmData::new();
        
        // Create test nodes for parallel ways
        data.add_node(crate::OsmNode::new(1, 0.0, 0.0));
        data.add_node(crate::OsmNode::new(2, 0.001, 0.0));
        data.add_node(crate::OsmNode::new(3, 0.0, 0.0001));
        data.add_node(crate::OsmNode::new(4, 0.001, 0.0001));
        // Perpendicular way nodes
        data.add_node(crate::OsmNode::new(5, 0.0005, -0.0001));
        data.add_node(crate::OsmNode::new(6, 0.0005, 0.0001));
        
        let road = crate::OsmWay::new(1, vec![1, 2]);
        
        // Parallel sidewalk
        let parallel_sidewalk = crate::OsmWay::new(2, vec![3, 4]);
        // Perpendicular crossing
        let perp_sidewalk = crate::OsmWay::new(3, vec![5, 6]);
        
        data.ways.insert(2, parallel_sidewalk.clone());
        data.ways.insert(3, perp_sidewalk.clone());
        
        let sidewalks: Vec<_> = vec![
            WayWithLineString {
                way: parallel_sidewalk,
                linestring: way_to_linestring(&data.ways[&2], &data).unwrap(),
            },
            WayWithLineString {
                way: perp_sidewalk,
                linestring: way_to_linestring(&data.ways[&3], &data).unwrap(),
            }
        ];
        let rtree = RTree::bulk_load(sidewalks);
        
        // Should find only the parallel one
        let candidates = find_parallel_adjacent_ways(&road, &rtree, 20.0, 15.0, &data);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].way.id, 2); // The parallel one
    }
}