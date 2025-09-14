//! Functions for detecting and analyzing OSM intersections.
//!
//! This module includes logic for finding topological intersections (shared nodes),
//! geometric intersections (unsplit ways), and clustering them into logical groups.

use crate::data::{OsmData, OsmWay};
use crate::graph::RoadGraph;
use geo::{Contains, Distance, Haversine, Point, Rect};
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

/// Unified, efficient intersection detection index.
/// 
/// This replaces multiple O(n²) functions with a single O(n) index build
/// and O(1) lookups for intersection queries.
#[derive(Debug)]
pub struct IntersectionIndex {
    /// Maps node ID to list of connected way IDs (O(1) lookup)
    node_to_ways: HashMap<i64, Vec<i64>>,
    /// Maps node ID to intersection degree (O(1) lookup)
    node_degrees: HashMap<i64, usize>,
    /// Intersection nodes with degree >= 3
    intersection_nodes: Vec<i64>,
    /// R-tree for efficient spatial queries
    spatial_index: RTree<IntersectionNodePoint>,
    /// Reference to OSM data for coordinate lookups
    data: OsmData,
}

impl IntersectionIndex {
    /// Build intersection index from OSM data with optional bounding box filter.
    /// 
    /// This is O(n) for building the index, then provides O(1) lookups.
    pub fn build(data: &OsmData, bbox: Option<Rect>) -> Self {
        // Build node → ways mapping (O(n))
        let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();
        for way in data.ways.values() {
            if is_road_way(way) {
                for &node_id in &way.nodes {
                    // Apply bbox filter if specified
                    if let Some(bbox) = bbox {
                        if let Some(node) = data.nodes.get(&node_id) {
                            if !bbox.contains(&Point::new(node.lon, node.lat)) {
                                continue;
                            }
                        }
                    }
                    node_to_ways.entry(node_id).or_default().push(way.id);
                }
            }
        }

        // Calculate degrees and find intersection nodes (O(n))
        let mut node_degrees = HashMap::new();
        let mut intersection_nodes = Vec::new();
        let mut spatial_points = Vec::new();

        for (&node_id, ways) in &node_to_ways {
            let degree = ways.len();
            node_degrees.insert(node_id, degree);
            
            if degree >= 3 {
                intersection_nodes.push(node_id);
                
                // Add to spatial index for clustering queries
                if let Some(node) = data.nodes.get(&node_id) {
                    spatial_points.push(IntersectionNodePoint {
                        node_id,
                        point: Point::new(node.lon, node.lat),
                        degree,
                    });
                }
            }
        }

        // Build R-tree for efficient spatial queries (O(n log n))
        let spatial_index = RTree::bulk_load(spatial_points);

        Self {
            node_to_ways,
            node_degrees,
            intersection_nodes,
            spatial_index,
            data: data.clone(),
        }
    }

    /// Get all intersection nodes (degree >= 3) - O(1)
    pub fn get_intersection_nodes(&self) -> &[i64] {
        &self.intersection_nodes
    }

    /// Get degree of a specific node - O(1)
    pub fn get_node_degree(&self, node_id: i64) -> usize {
        self.node_degrees.get(&node_id).copied().unwrap_or(0)
    }

    /// Get ways connected to a specific node - O(1)
    pub fn get_connected_ways(&self, node_id: i64) -> &[i64] {
        self.node_to_ways.get(&node_id).map_or(&[], |v| v.as_slice())
    }

    /// Get logical intersections by clustering nearby intersection nodes - O(n log n)
    pub fn get_logical_intersections(
        &self,
        max_cluster_distance: f64,
        topology: Option<(&RoadGraph, &HashMap<i64, NodeIndex>)>,
    ) -> Vec<LogicalIntersection> {
        if self.intersection_nodes.is_empty() {
            return Vec::new();
        }

        let mut clusters: Vec<Vec<i64>> = Vec::new();
        let mut used_nodes = HashSet::new();

        for &node_id in &self.intersection_nodes {
            if used_nodes.contains(&node_id) {
                continue;
            }

            let mut cluster = vec![node_id];
            used_nodes.insert(node_id);
            let mut queue = vec![node_id];

            while let Some(current_node_id) = queue.pop() {
                // Spatial clustering
                if let Some(node) = self.data.nodes.get(&current_node_id) {
                    let query_point = Point::new(node.lon, node.lat);
                    let buffer_degrees = max_cluster_distance / 111_320.0;
                    let search_envelope = AABB::from_corners(
                        (
                            query_point.x() - buffer_degrees,
                            query_point.y() - buffer_degrees,
                        )
                            .into(),
                        (
                            query_point.x() + buffer_degrees,
                            query_point.y() + buffer_degrees,
                        )
                            .into(),
                    );

                    for candidate in self
                        .spatial_index
                        .locate_in_envelope_intersecting(&search_envelope)
                    {
                        if !used_nodes.contains(&candidate.node_id)
                            && Haversine.distance(query_point, candidate.point)
                                <= max_cluster_distance
                        {
                            used_nodes.insert(candidate.node_id);
                            queue.push(candidate.node_id);
                            cluster.push(candidate.node_id);
                        }
                    }
                }

                // Topological clustering
                if let Some((graph, node_map)) = topology {
                    if let Some(&graph_index) = node_map.get(&current_node_id) {
                        let mut neighbors = graph.neighbors_undirected(graph_index).detach();
                        while let Some((edge_index, neighbor_graph_index)) = neighbors.next(graph) {
                            let neighbor_node_id = graph[neighbor_graph_index].osm_node_id;
                            if !used_nodes.contains(&neighbor_node_id) {
                                if let Some(edge) = graph.edge_weight(edge_index) {
                                    if edge.length_meters <= max_cluster_distance {
                                        used_nodes.insert(neighbor_node_id);
                                        queue.push(neighbor_node_id);
                                        cluster.push(neighbor_node_id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            clusters.push(cluster);
        }

        clusters
            .into_iter()
            .map(|mut cluster| {
                cluster.sort(); // Sort for consistent representative node
                LogicalIntersection {
                    representative_node: cluster[0],
                    node_cluster: cluster,
                }
            })
            .collect()
    }

    /// Get OSM data reference
    pub fn data(&self) -> &OsmData {
        &self.data
    }
}

/// Build a fast lookup index from nodes to connected ways.
/// 
/// This function is now provided by IntersectionIndex::build(), but kept
/// for compatibility with existing code.
pub fn build_node_to_ways_index(data: &OsmData) -> HashMap<i64, Vec<i64>> {
    let mut node_to_ways: HashMap<i64, Vec<i64>> = HashMap::new();
    for way in data.ways.values() {
        if is_road_way(way) {
            for &node_id in &way.nodes {
                node_to_ways.entry(node_id).or_default().push(way.id);
            }
        }
    }
    node_to_ways
}

/// Check if a way represents a road that can have intersections.
pub fn is_road_way(way: &OsmWay) -> bool {
    way.tags.get("highway").is_some_and(|h| {
        matches!(
            h.as_str(),
            "primary"
                | "secondary"
                | "tertiary"
                | "unclassified"
                | "residential"
                | "trunk"
                | "motorway"
        )
    })
}

/// Find all intersections in a road network (degree 3+).
/// 
/// **DEPRECATED**: Use `IntersectionIndex::get_intersection_nodes()` instead.
/// This function is kept for backward compatibility but may be removed in future versions.
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

/// Find logical intersections by clustering nearby intersection nodes (RTree optimized).
/// 
/// **DEPRECATED**: Use `IntersectionIndex::get_logical_intersections()` instead.
/// This function is kept for backward compatibility but may be removed in future versions.
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
                    (
                        query_point.x() - buffer_degrees,
                        query_point.y() - buffer_degrees,
                    )
                        .into(),
                    (
                        query_point.x() + buffer_degrees,
                        query_point.y() + buffer_degrees,
                    )
                        .into(),
                );

                for candidate in rtree.locate_in_envelope_intersecting(&search_envelope) {
                    if !used_nodes.contains(&candidate.node_id)
                        && Haversine.distance(query_point, candidate.point) <= max_cluster_distance
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

/// Extract comprehensive metadata for a logical intersection.
pub fn extract_intersection_metadata(
    logical_intersection: &LogicalIntersection,
    data: &OsmData,
    node_to_ways: &HashMap<i64, Vec<i64>>,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
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

    let ways: Vec<_> = connected_ways
        .iter()
        .filter_map(|id| data.ways.get(id))
        .collect();
    metadata.insert("degree".into(), ways.len().to_string());
    metadata.insert(
        "connected_ways".into(),
        ways.iter()
            .map(|w| w.id.to_string())
            .collect::<Vec<_>>()
            .join(","),
    );

    let highway_types: Vec<_> = ways
        .iter()
        .filter_map(|w| w.tags.get("highway").cloned())
        .collect();
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
            data.ways.get(way_id).map_or(false, |w| {
                w.tags.get("junction") == Some(&"roundabout".to_string())
            })
        });

        if degree >= 4 || has_roundabout {
            complex_intersections.push(osm_node_id);
        }
    }
    complex_intersections
}

/// Intersection representation with full details.
#[derive(Debug, Clone)]
pub struct Intersection {
    pub representative_node: i64,
    pub node_cluster: Vec<i64>,
    pub degree: usize,
    pub intersection_type: String,
    pub coordinates: Point<f64>,
}

/// Analyzer for processing intersection data into structured results.
pub struct IntersectionAnalyzer {
    cluster_distance: f64,
}

impl IntersectionAnalyzer {
    pub fn new(cluster_distance: f64) -> Self {
        Self { cluster_distance }
    }

    pub fn analyze(&self, osm_data: &OsmData) -> Vec<Intersection> {
        // Build the road graph to get topological information
        let (road_graph, node_map) = crate::graph::build_road_network(osm_data);

        // Use the new unified IntersectionIndex for efficient processing
        let intersection_index = IntersectionIndex::build(osm_data, None);
        let logical_intersections =
            intersection_index.get_logical_intersections(self.cluster_distance, Some((&road_graph, &node_map)));

        // Process the results into the final `Intersection` struct
        logical_intersections
            .into_iter()
            .map(|li| {
                let mut connected_ways = HashSet::new();
                for &node_id in &li.node_cluster {
                    for &way_id in intersection_index.get_connected_ways(node_id) {
                        connected_ways.insert(way_id);
                    }
                }
                let degree = connected_ways.len();

                let intersection_type = if li.node_cluster.len() > 1 {
                    "complex"
                } else {
                    "simple"
                }
                .to_string();

                let representative_node_data = osm_data.nodes.get(&li.representative_node).unwrap();

                Intersection {
                    representative_node: li.representative_node,
                    node_cluster: li.node_cluster,
                    degree,
                    intersection_type,
                    coordinates: Point::new(
                        representative_node_data.lon,
                        representative_node_data.lat,
                    ),
                }
            })
            .collect()
    }
}
