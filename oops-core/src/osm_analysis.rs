//! OSM-specific analysis utilities
//!
//! Functions for analyzing OpenStreetMap data structures and relationships

use crate::{OsmData, OsmWay};
use geo::GeodesicLength;
use geo::*;
use petgraph::graph::{Graph, NodeIndex};
use petgraph::Direction;
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
            (
                target_envelope.lower().x() - buffer_distance,
                target_envelope.lower().y() - buffer_distance,
            )
                .into(),
            (
                target_envelope.upper().x() + buffer_distance,
                target_envelope.upper().y() + buffer_distance,
            )
                .into(),
        );
        candidate_rtree
            .locate_in_envelope_intersecting(&bigger_envelope)
            .filter(|candidate| {
                // Calculate minimum distance between linestrings
                let min_dist = candidate
                    .linestring
                    .points()
                    .map(|p1| {
                        target_linestring
                            .points()
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
            let is_parallel =
                angle_diff <= max_angle_diff || (angle_diff - 180.0).abs() <= max_angle_diff;

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

    for i in 0..points.len() - 1 {
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
                target_points[segment_index]
            };

            // Check if this sample point has nearby sidewalk coverage
            // Use point-to-line-segment distance for more accurate results
            let has_coverage = parallel_sidewalks.iter().any(|sidewalk| {
                let min_dist =
                    calculate_point_to_linestring_distance(&sample_point, &sidewalk.linestring);
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
pub fn calculate_point_to_linestring_distance(
    point: &geo::Point,
    linestring: &geo::LineString,
) -> f64 {
    let points: Vec<_> = linestring.points().collect();
    if points.len() < 2 {
        // Degenerate case - treat as point-to-point distance
        return points.first()
            .map_or(f64::MAX, |p| point.haversine_distance(p));
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
fn calculate_point_to_segment_distance(
    point: &geo::Point,
    seg_start: &geo::Point,
    seg_end: &geo::Point,
) -> f64 {
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
    let mut node_way_count: HashMap<i64, usize> = HashMap::new();

    // This is the expensive part that iterates over all ways.
    // It's unavoidable without a different data structure (e.g. node->way index).
    for way in data.ways.values() {
        if is_road_way(way) {
            for &node_id in &way.nodes {
                *node_way_count.entry(node_id).or_insert(0) += 1;
            }
        }
    }

    // Filter the nodes that are intersections AND are inside the bbox.
    node_way_count
        .into_iter()
        .filter_map(|(node_id, count)| {
            if count >= 3 {
                if let Some(node) = data.nodes.get(&node_id) {
                    let point = Point::new(node.lon, node.lat);
                    if bbox.contains(&point) {
                        return Some(node_id);
                    }
                }
            }
            None
        })
        .collect()
}

/// Represents a logical intersection as a cluster of related intersection nodes
#[derive(Debug, Clone)]
pub struct LogicalIntersection {
    /// Representative node ID for this logical intersection
    pub representative_node: i64,
    /// All intersection nodes that belong to this logical intersection
    pub node_cluster: Vec<i64>,
}

/// Wrapper for intersection nodes to use with RTree (following WayWithLineString pattern)
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

/// Find logical intersections by clustering nearby intersection nodes (RTree optimized)
///
/// Groups intersection nodes within a distance threshold into logical intersections.
/// This handles complex cases like divided highway intersections and large roundabouts.
/// Uses RTree spatial indexing for O(n log n) performance instead of O(n²).
///
/// # Arguments
/// * `data` - OSM data containing ways and nodes
/// * `max_cluster_distance` - Maximum distance in meters to group nodes (default: 100m)
///
/// # Returns
/// Vector of LogicalIntersection structs, each representing one logical intersection
pub fn find_logical_intersections(
    data: &OsmData,
    max_cluster_distance: f64,
) -> Vec<LogicalIntersection> {
    let intersection_nodes = find_road_intersections(data);

    if intersection_nodes.is_empty() {
        return Vec::new();
    }

    // Build RTree of intersection nodes for fast spatial queries
    let node_points: Vec<IntersectionNodePoint> = intersection_nodes
        .iter()
        .filter_map(|&node_id| {
            data.nodes.get(&node_id).map(|node| IntersectionNodePoint {
                node_id,
                point: Point::new(node.lon, node.lat),
                degree: 0, // Will be calculated properly in the intersection analyzer
            })
        })
        .collect();

    let rtree = RTree::bulk_load(node_points);

    // Create clusters using RTree for fast neighbor finding
    let mut clusters: Vec<Vec<i64>> = Vec::new();
    let mut used_nodes = std::collections::HashSet::new();

    for &node_id in &intersection_nodes {
        if used_nodes.contains(&node_id) {
            continue; // Already assigned to a cluster
        }

        let mut cluster = vec![node_id];
        used_nodes.insert(node_id);

        if let Some(node) = data.nodes.get(&node_id) {
            let query_point = Point::new(node.lon, node.lat);

            // Create search envelope around the node (reusing pattern from find_adjacent_way_candidates)
            let buffer_degrees = max_cluster_distance / 111_320.0; // Rough degrees conversion
            let search_envelope = AABB::from_corners(
                Point::new(
                    query_point.x() - buffer_degrees,
                    query_point.y() - buffer_degrees,
                ),
                Point::new(
                    query_point.x() + buffer_degrees,
                    query_point.y() + buffer_degrees,
                ),
            );

            // Find nearby intersection nodes using RTree
            let nearby_candidates: Vec<_> = rtree
                .locate_in_envelope_intersecting(&search_envelope)
                .filter(|candidate| {
                    !used_nodes.contains(&candidate.node_id)
                        && query_point.haversine_distance(&candidate.point) <= max_cluster_distance
                })
                .collect();

            // Add nearby nodes to cluster
            for candidate in nearby_candidates {
                cluster.push(candidate.node_id);
                used_nodes.insert(candidate.node_id);
            }
        }

        clusters.push(cluster);
    }

    // Convert clusters to LogicalIntersection structs
    clusters
        .into_iter()
        .map(|cluster| LogicalIntersection {
            representative_node: cluster[0], // Simple: just use first node as representative
            node_cluster: cluster,
        })
        .collect()
}

/// Find logical intersections within a geographic bounding box
///
/// # Arguments
/// * `data` - OSM data containing ways and nodes
/// * `bbox` - Bounding rectangle to filter intersections
/// * `max_cluster_distance` - Maximum distance in meters to group nodes (default: 100m)
///
/// # Returns
/// Vector of LogicalIntersection structs within the bbox
pub fn find_logical_intersections_in_bbox(
    data: &OsmData,
    bbox: Rect,
    max_cluster_distance: f64,
) -> Vec<LogicalIntersection> {
    // 1. Define a search bbox that is the original bbox expanded by max_cluster_distance.
    let buffer_degrees = max_cluster_distance / 111_320.0; // Rough conversion
    let search_bbox = Rect::new(
        Coord {
            x: bbox.min().x - buffer_degrees,
            y: bbox.min().y - buffer_degrees,
        },
        Coord {
            x: bbox.max().x + buffer_degrees,
            y: bbox.max().y + buffer_degrees,
        },
    );

    // 2. Find intersection nodes only within this larger search area.
    let intersection_nodes = find_road_intersections_in_bbox(data, search_bbox);

    if intersection_nodes.is_empty() {
        return Vec::new();
    }

    // 3. Build RTree and cluster only this subset of nodes.
    let node_points: Vec<IntersectionNodePoint> = intersection_nodes
        .iter()
        .filter_map(|&node_id| {
            data.nodes.get(&node_id).map(|node| IntersectionNodePoint {
                node_id,
                point: Point::new(node.lon, node.lat),
                degree: 0, // Will be calculated properly in the intersection analyzer
            })
        })
        .collect();

    let rtree = RTree::bulk_load(node_points.clone());

    let mut clusters: Vec<Vec<i64>> = Vec::new();
    let mut visited_nodes = std::collections::HashSet::new();
    let node_point_map: HashMap<_, _> = node_points.iter().map(|p| (p.node_id, p.point)).collect();

    for node_point in &node_points {
        let start_node_id = node_point.node_id;
        if visited_nodes.contains(&start_node_id) {
            continue;
        }

        let mut current_cluster = Vec::new();
        let mut queue = std::collections::VecDeque::new();

        queue.push_back(start_node_id);
        visited_nodes.insert(start_node_id);

        while let Some(node_id) = queue.pop_front() {
            current_cluster.push(node_id);

            if let Some(current_node_point) = node_point_map.get(&node_id) {
                let buffer_degrees_cluster = max_cluster_distance / 111_320.0;
                let search_envelope = AABB::from_corners(
                    (
                        current_node_point.x() - buffer_degrees_cluster,
                        current_node_point.y() - buffer_degrees_cluster,
                    )
                        .into(),
                    (
                        current_node_point.x() + buffer_degrees_cluster,
                        current_node_point.y() + buffer_degrees_cluster,
                    )
                        .into(),
                );

                for neighbor in rtree.locate_in_envelope_intersecting(&search_envelope) {
                    if !visited_nodes.contains(&neighbor.node_id) && current_node_point.haversine_distance(&neighbor.point) <= max_cluster_distance {
                        visited_nodes.insert(neighbor.node_id);
                        queue.push_back(neighbor.node_id);
                    }
                }
            }
        }
        clusters.push(current_cluster);
    }

    // 4. Convert clusters to LogicalIntersection and filter by the original bbox.
    clusters
        .into_iter()
        .filter_map(|mut cluster| {
            // Sort cluster nodes for consistent representative node selection
            cluster.sort();
            let representative_node = *cluster.first()?;

            if let Some(node) = data.nodes.get(&representative_node) {
                let point = Point::new(node.lon, node.lat);
                if bbox.contains(&point) {
                    Some(LogicalIntersection {
                        representative_node,
                        node_cluster: cluster,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// Build a fast lookup index from nodes to connected ways
/// This eliminates the need to iterate through all ways for each node lookup
pub fn build_node_to_ways_index(data: &OsmData) -> HashMap<i64, Vec<i64>> {
    let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();

    for way in data.ways.values() {
        if is_road_way(way) {
            for &node_id in &way.nodes {
                node_to_ways
                    .entry(node_id)
                    .or_default()
                    .push(way.id);
            }
        }
    }

    node_to_ways
}

/// Extract comprehensive metadata for a logical intersection (optimized version)
///
/// Analyzes all nodes in the intersection cluster and all connected ways
/// to provide rich metadata about the intersection as a whole.
/// Uses pre-built index for O(1) node-to-ways lookup instead of O(n) iteration.
///
/// # Arguments
/// * `logical_intersection` - The logical intersection to analyze
/// * `data` - OSM data for lookups
/// * `node_to_ways` - Pre-built index from node IDs to connected way IDs
///
/// # Returns
/// HashMap with intersection metadata for use in AnalysisResult context
pub fn extract_intersection_metadata_optimized(
    logical_intersection: &LogicalIntersection,
    data: &OsmData,
    node_to_ways: &HashMap<i64, Vec<i64>>,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();

    // Basic cluster information
    metadata.insert(
        "representative_node".into(),
        logical_intersection.representative_node.to_string(),
    );
    metadata.insert(
        "cluster_size".into(),
        logical_intersection.node_cluster.len().to_string(),
    );
    metadata.insert(
        "node_cluster".into(),
        logical_intersection
            .node_cluster
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(","),
    );

    // Get coordinates of representative node
    if let Some(node) = data.nodes.get(&logical_intersection.representative_node) {
        metadata.insert("coordinates".into(), format!("{},{}", node.lat, node.lon));
    }

    // Find all ways connected to ANY node in the cluster using fast lookup
    let mut connected_ways: Vec<&OsmWay> = Vec::new();
    let mut way_ids_seen = std::collections::HashSet::new();

    for &node_id in &logical_intersection.node_cluster {
        if let Some(way_ids) = node_to_ways.get(&node_id) {
            for &way_id in way_ids {
                if !way_ids_seen.contains(&way_id) {
                    if let Some(way) = data.ways.get(&way_id) {
                        connected_ways.push(way);
                        way_ids_seen.insert(way_id);
                    }
                }
            }
        }
    }

    // Extract way information
    metadata.insert("degree".into(), connected_ways.len().to_string());
    metadata.insert(
        "connected_ways".into(),
        connected_ways
            .iter()
            .map(|way| way.id.to_string())
            .collect::<Vec<_>>()
            .join(","),
    );

    // Analyze highway types
    let highway_types: Vec<String> = connected_ways
        .iter()
        .filter_map(|way| way.tags.get("highway")).cloned()
        .collect();
    metadata.insert("highway_types".into(), highway_types.join(","));

    // Count unique highway types
    let unique_types: std::collections::HashSet<_> = highway_types.iter().collect();
    metadata.insert("highway_type_count".into(), unique_types.len().to_string());

    // Analyze way names
    let way_names: Vec<String> = connected_ways
        .iter()
        .map(|way| way.tags.get("name").cloned().unwrap_or_default())
        .collect();
    metadata.insert("way_names".into(), way_names.join(","));

    let named_ways_count = way_names.iter().filter(|name| !name.is_empty()).count();
    metadata.insert("named_ways".into(), named_ways_count.to_string());
    metadata.insert("total_ways".into(), connected_ways.len().to_string());

    // Determine intersection complexity
    let complexity = if connected_ways.len() >= 5 {
        "very_complex"
    } else if connected_ways.len() >= 4 {
        "complex"
    } else if connected_ways.len() == 3 {
        "simple"
    } else {
        "minimal"
    };
    metadata.insert("complexity".into(), complexity.into());

    // Check for special junction types
    let mut junction_types = Vec::new();
    for way in &connected_ways {
        if let Some(junction) = way.tags.get("junction") {
            junction_types.push(junction.clone());
        }
    }

    if !junction_types.is_empty() {
        metadata.insert("junction_types".into(), junction_types.join(","));
        metadata.insert(
            "has_roundabout".into(),
            junction_types.iter().any(|j| j == "roundabout").to_string(),
        );
    }

    // Check for traffic control
    let has_traffic_signals = logical_intersection.node_cluster.iter().any(|&node_id| {
        if let Some(node) = data.nodes.get(&node_id) {
            node.tags.get("highway") == Some(&"traffic_signals".into())
        } else {
            false
        }
    });
    metadata.insert(
        "has_traffic_signals".into(),
        has_traffic_signals.to_string(),
    );

    // Analyze road importance (highest classification present)
    let road_importance = highway_types
        .iter()
        .map(|highway_type| match highway_type.as_str() {
            "motorway" => 7,
            "trunk" => 6,
            "primary" => 5,
            "secondary" => 4,
            "tertiary" => 3,
            "unclassified" => 2,
            "residential" => 1,
            _ => 0,
        })
        .max()
        .unwrap_or(0);

    let importance_name = match road_importance {
        7 => "motorway",
        6 => "trunk",
        5 => "primary",
        4 => "secondary",
        3 => "tertiary",
        2 => "unclassified",
        1 => "residential",
        _ => "unknown",
    };
    metadata.insert("max_road_importance".into(), importance_name.into());

    metadata
}

/// Extract comprehensive metadata for a logical intersection (legacy version)
///
/// Analyzes all nodes in the intersection cluster and all connected ways
/// to provide rich metadata about the intersection as a whole.
///
/// # Arguments
/// * `logical_intersection` - The logical intersection to analyze
/// * `data` - OSM data for lookups
///
/// # Returns
/// HashMap with intersection metadata for use in AnalysisResult context
pub fn extract_intersection_metadata(
    logical_intersection: &LogicalIntersection,
    data: &OsmData,
) -> HashMap<String, String> {
    // Build the node-to-ways index on demand for backward compatibility
    let node_to_ways = build_node_to_ways_index(data);
    extract_intersection_metadata_optimized(logical_intersection, data, &node_to_ways)
}

/// Find complex intersections (roundabouts, multi-way junctions) using graph-based analysis
///
/// These require special handling for pedestrian crossing analysis
///
/// # Arguments
/// * `data` - OSM data
/// * `graph` - Road network graph
/// * `node_map` - Mapping from OSM node IDs to graph indices
///
/// # Returns
/// Vector of complex intersection node IDs (degree 4+, roundabouts)
pub fn find_complex_intersections_graph(
    data: &OsmData,
    graph: &RoadGraph,
    node_map: &HashMap<i64, NodeIndex>,
) -> Vec<i64> {
    let mut complex_intersections = Vec::new();

    for (&osm_node_id, &graph_idx) in node_map {
        // Calculate degree as number of unique connected ways (not total directed edges)
        let mut connected_ways: std::collections::HashSet<i64> = std::collections::HashSet::new();

        // Collect way IDs from both outgoing and incoming edges
        for edge_ref in graph.edges_directed(graph_idx, Direction::Outgoing) {
            connected_ways.insert(edge_ref.weight().osm_way_id);
        }
        for edge_ref in graph.edges_directed(graph_idx, Direction::Incoming) {
            connected_ways.insert(edge_ref.weight().osm_way_id);
        }

        let degree = connected_ways.len();

        // Check for roundabouts by examining connected ways
        let has_roundabout = graph
            .edges_directed(graph_idx, Direction::Outgoing)
            .chain(graph.edges_directed(graph_idx, Direction::Incoming))
            .any(|edge_ref| {
                let segment = edge_ref.weight();
                // Check if the original way was a roundabout
                if let Some(way) = data.ways.get(&segment.osm_way_id) {
                    way.tags.get("junction").is_some_and(|j| j == "roundabout")
                } else {
                    false
                }
            });

        // Complex if 4+ degree or contains roundabout
        if degree >= 4 || has_roundabout {
            complex_intersections.push(osm_node_id);
        }
    }

    complex_intersections
}

/// Legacy function - use find_complex_intersections_graph for better performance
pub fn find_complex_intersections(data: &OsmData, intersection_nodes: &[i64]) -> Vec<i64> {
    let mut complex_intersections = Vec::new();

    for &node_id in intersection_nodes {
        let connected_ways: Vec<_> = data
            .ways
            .values()
            .filter(|way| is_road_way(way) && way.nodes.contains(&node_id))
            .collect();

        // Complex if 4+ ways or contains roundabout
        if connected_ways.len() >= 4
            || connected_ways
                .iter()
                .any(|way| way.tags.get("junction").is_some_and(|j| j == "roundabout"))
        {
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

/// Builds a graph representation of the road network from OSM data.
///
/// Nodes in the graph are intersections (or dead-ends), and edges are the
/// road segments connecting them.
///
/// # Arguments
/// * `data` - The OSM data to process.
///
/// # Returns
/// A `petgraph::Graph` and a `HashMap` mapping OSM node IDs to graph `NodeIndex`.
// Data stored for each node (intersection) in the graph
#[derive(Debug, Clone)]
pub struct IntersectionGraphNode {
    pub osm_node_id: i64,
    pub point: geo::Point,
}

// Data stored for each edge (road segment) in the graph
#[derive(Debug, Clone)]
pub struct RoadSegment {
    pub osm_way_id: i64,
    pub geometry: geo::LineString,
    pub length_meters: f64,
    // Using a smaller set of tags for now to avoid duplicating too much data
    pub highway_type: Option<String>,
    pub name: Option<String>,
}

pub type RoadGraph = Graph<IntersectionGraphNode, RoadSegment, petgraph::Directed>;

/// Builds a graph representation of the road network from OSM data.
///
/// Nodes in the graph are intersections (or dead-ends), and edges are the
/// road segments connecting them.
///
/// # Arguments
/// * `data` - The OSM data to process.
///
/// # Returns
/// A `petgraph::Graph` and a `HashMap` mapping OSM node IDs to graph `NodeIndex`.
pub fn build_road_network(data: &OsmData) -> (RoadGraph, HashMap<i64, NodeIndex>) {
    let mut graph = RoadGraph::new();
    let mut osm_node_to_graph_index = HashMap::new();
    let node_to_ways = build_node_to_ways_index(data);

    // 1. Identify junction nodes (intersections or dead-ends) and add them to the graph
    let mut junction_nodes = std::collections::HashSet::new();
    for (node_id, ways) in &node_to_ways {
        if ways.len() > 1 {
            // Node is in multiple ways, it's a true intersection.
            junction_nodes.insert(*node_id);
        } else if ways.len() == 1 {
            // Node is in a single way. Check if it's a dead-end.
            let way = &data.ways[&ways[0]];
            if !way.is_closed() {
                if let Some(&first_node) = way.nodes.first() {
                    if first_node == *node_id {
                        junction_nodes.insert(*node_id);
                    }
                }
                if let Some(&last_node) = way.nodes.last() {
                    if last_node == *node_id {
                        junction_nodes.insert(*node_id);
                    }
                }
            }
        }
    }

    for &node_id in &junction_nodes {
        if let Some(node) = data.nodes.get(&node_id) {
            let graph_node = IntersectionGraphNode {
                osm_node_id: node.id,
                point: Point::new(node.lon, node.lat),
            };
            let index = graph.add_node(graph_node);
            osm_node_to_graph_index.insert(node.id, index);
        }
    }

    // 2. Iterate through each road way and create edges by splitting the way at junctions
    for way in data.ways.values() {
        if !is_road_way(way) {
            continue;
        }

        let mut segment_start_index = 0;
        for i in 1..way.nodes.len() {
            let node_id = way.nodes[i];
            if junction_nodes.contains(&node_id) {
                // We've found a junction. The path from the previous junction
                // to this one forms an edge.
                let segment_nodes = &way.nodes[segment_start_index..=i];
                add_edge_from_path(
                    &mut graph,
                    &osm_node_to_graph_index,
                    segment_nodes,
                    way,
                    data,
                );
                segment_start_index = i;
            }
        }
    }

    (graph, osm_node_to_graph_index)
}

/// Helper to create a RoadSegment and add it as an edge to the graph
fn add_edge_from_path(
    graph: &mut RoadGraph,
    node_map: &HashMap<i64, NodeIndex>,
    path_nodes: &[i64],
    way: &OsmWay,
    data: &OsmData,
) {
    if path_nodes.len() < 2 {
        return;
    }

    let start_node_id = path_nodes[0];
    let end_node_id = path_nodes[path_nodes.len() - 1];

    if let (Some(&start_index), Some(&end_index)) =
        (node_map.get(&start_node_id), node_map.get(&end_node_id))
    {
        if start_index == end_index {
            return; // Avoid loops
        }

        let coords: Vec<_> = path_nodes
            .iter()
            .filter_map(|&id| data.nodes.get(&id))
            .map(|node| Coord {
                x: node.lon,
                y: node.lat,
            })
            .collect();

        if coords.len() < 2 {
            return;
        }

        let geometry = LineString::new(coords);
        let length_meters = geometry.geodesic_length();

        let segment = RoadSegment {
            osm_way_id: way.id,
            geometry,
            length_meters,
            highway_type: way.tags.get("highway").cloned(),
            name: way.tags.get("name").cloned(),
        };

        // Handle oneway directionality for directed graph
        let oneway = way.tags.get("oneway").map(|s| s.as_str());
        let is_roundabout = way
            .tags
            .get("junction")
            .is_some_and(|j| j == "roundabout");

        let forward =
            oneway == Some("yes") || oneway == Some("true") || oneway == Some("1") || is_roundabout;
        let backward = oneway == Some("-1");

        if !forward && !backward {
            // Add edges in both directions if not a oneway
            if graph.find_edge(start_index, end_index).is_none() {
                graph.add_edge(start_index, end_index, segment.clone());
            }
            if graph.find_edge(end_index, start_index).is_none() {
                graph.add_edge(end_index, start_index, segment);
            }
        } else if forward {
            // Add only forward edge
            if graph.find_edge(start_index, end_index).is_none() {
                graph.add_edge(start_index, end_index, segment);
            }
        } else if backward {
            // Add only backward edge
            if graph.find_edge(end_index, start_index).is_none() {
                graph.add_edge(end_index, start_index, segment);
            }
        }
    }
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
    fn test_build_road_network_with_oneway() {
        let mut data = OsmData::new();

        // Create test nodes for a simple road network
        data.add_node(crate::OsmNode::new(1, 0.0, 0.0));
        data.add_node(crate::OsmNode::new(2, 0.001, 0.0));
        data.add_node(crate::OsmNode::new(3, 0.002, 0.0));

        // Regular bidirectional road from node 1 to 2
        let mut way1 = crate::OsmWay::new(1, vec![1, 2]);
        way1.tags.insert("highway".into(), "residential".into());
        data.ways.insert(1, way1);

        // Oneway road from node 2 to 3
        let mut way2 = crate::OsmWay::new(2, vec![2, 3]);
        way2.tags.insert("highway".into(), "residential".into());
        way2.tags.insert("oneway".into(), "yes".into());
        data.ways.insert(2, way2);

        let (graph, node_map) = build_road_network(&data);

        let node1_idx = node_map[&1];
        let node2_idx = node_map[&2];
        let node3_idx = node_map[&3];

        // Check bidirectional road: should have edges both ways
        assert!(graph.find_edge(node1_idx, node2_idx).is_some());
        assert!(graph.find_edge(node2_idx, node1_idx).is_some());

        // Check oneway road: should have edge only in forward direction
        assert!(graph.find_edge(node2_idx, node3_idx).is_some());
        assert!(graph.find_edge(node3_idx, node2_idx).is_none());
    }

    #[test]
    fn test_build_road_network_with_roundabout() {
        let mut data = OsmData::new();

        // Create nodes for a roundabout with connecting roads
        data.add_node(crate::OsmNode::new(1, 0.0, 0.0)); // roundabout node
        data.add_node(crate::OsmNode::new(2, 0.001, 0.0)); // roundabout node
        data.add_node(crate::OsmNode::new(3, 0.001, 0.001)); // roundabout node
        data.add_node(crate::OsmNode::new(4, 0.0, 0.001)); // roundabout node
        data.add_node(crate::OsmNode::new(5, -0.001, 0.0)); // connecting road end

        // Roundabout way (automatically oneway)
        let mut roundabout = crate::OsmWay::new(1, vec![1, 2, 3, 4, 1]);
        roundabout.tags.insert("highway".into(), "primary".into());
        roundabout
            .tags
            .insert("junction".into(), "roundabout".into());
        data.ways.insert(1, roundabout);

        // Connecting road to make node 1 a junction
        let mut connecting_road = crate::OsmWay::new(2, vec![5, 1]);
        connecting_road
            .tags
            .insert("highway".into(), "primary".into());
        data.ways.insert(2, connecting_road);

        let (graph, node_map) = build_road_network(&data);

        // Only node 1 should be in the graph (it's the junction where connecting road meets roundabout)
        assert!(node_map.contains_key(&1));
        assert!(node_map.contains_key(&5)); // dead-end of connecting road

        let node1_idx = node_map[&1];
        let node5_idx = node_map[&5];

        // Should have bidirectional edge for the connecting road
        assert!(graph.find_edge(node5_idx, node1_idx).is_some());
        assert!(graph.find_edge(node1_idx, node5_idx).is_some());
    }

    #[test]
    fn test_find_complex_intersections_graph() {
        let mut data = OsmData::new();

        // Create nodes for a complex 4-way intersection
        data.add_node(crate::OsmNode::new(1, 0.0, 0.0)); // Center
        data.add_node(crate::OsmNode::new(2, 0.001, 0.0)); // East
        data.add_node(crate::OsmNode::new(3, -0.001, 0.0)); // West
        data.add_node(crate::OsmNode::new(4, 0.0, 0.001)); // North
        data.add_node(crate::OsmNode::new(5, 0.0, -0.001)); // South

        // Simple T-junction
        data.add_node(crate::OsmNode::new(6, 0.002, 0.0)); // Another center
        data.add_node(crate::OsmNode::new(7, 0.003, 0.0)); // East of 6
        data.add_node(crate::OsmNode::new(8, 0.002, 0.001)); // North of 6

        // Ways forming 4-way intersection at node 1 (4 separate ways)
        let mut way1 = crate::OsmWay::new(1, vec![2, 1]); // East
        way1.tags.insert("highway".into(), "primary".into());
        data.ways.insert(1, way1);

        let mut way2 = crate::OsmWay::new(2, vec![1, 3]); // West
        way2.tags.insert("highway".into(), "primary".into());
        data.ways.insert(2, way2);

        let mut way3 = crate::OsmWay::new(3, vec![4, 1]); // North
        way3.tags.insert("highway".into(), "primary".into());
        data.ways.insert(3, way3);

        let mut way4 = crate::OsmWay::new(4, vec![1, 5]); // South
        way4.tags.insert("highway".into(), "primary".into());
        data.ways.insert(4, way4);

        // Ways forming T-junction at node 6 (only 3 ways = simple)
        let mut way6 = crate::OsmWay::new(6, vec![6, 7]); // East from 6
        way6.tags.insert("highway".into(), "residential".into());
        data.ways.insert(6, way6);

        let mut way7 = crate::OsmWay::new(7, vec![6, 8]); // North from 6
        way7.tags.insert("highway".into(), "residential".into());
        data.ways.insert(7, way7);

        let mut way5 = crate::OsmWay::new(5, vec![1, 6]); // Connect intersections
        way5.tags.insert("highway".into(), "residential".into());
        data.ways.insert(5, way5);

        let (graph, node_map) = build_road_network(&data);
        let complex_intersections = find_complex_intersections_graph(&data, &graph, &node_map);

        // Node 1 should be complex (5 ways >= 4), node 6 should be simple (3 ways)
        assert!(
            complex_intersections.contains(&1),
            "Node 1 should be complex (5 ways)"
        );
        assert!(
            !complex_intersections.contains(&6),
            "Node 6 should be simple (3 ways)"
        );
        assert_eq!(complex_intersections.len(), 1);
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
            },
        ];
        let rtree = RTree::bulk_load(sidewalks);

        // Should find only the parallel one
        let candidates = find_parallel_adjacent_ways(&road, &rtree, 20.0, 15.0, &data);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].way.id, 2); // The parallel one
    }
}
