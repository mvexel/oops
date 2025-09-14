//! Graph representation of the OSM road network.
//!
//! This module provides functions to build a `petgraph` from OSM data,
//! where nodes are intersections and edges are road segments.

use crate::data::{OsmData, OsmWay};
use crate::intersections::is_road_way;
use geo::{Geodesic, Length};
use petgraph::graph::{Graph, NodeIndex};
use std::collections::HashMap;

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
    pub highway_type: Option<String>,
    pub name: Option<String>,
    /// Starting node ID in the original OSM way
    pub start_node_id: i64,
    /// Ending node ID in the original OSM way  
    pub end_node_id: i64,
    /// Number of intermediate nodes between start and end
    pub intermediate_nodes: usize,
}

pub type RoadGraph = Graph<IntersectionGraphNode, RoadSegment, petgraph::Directed>;

/// Builds a graph representation of the road network from OSM data.
pub fn build_road_network(data: &OsmData) -> (RoadGraph, HashMap<i64, NodeIndex>) {
    let mut graph = RoadGraph::new();
    let mut osm_node_to_graph_index = HashMap::new();
    let node_to_ways = crate::intersections::build_node_to_ways_index(data);

    // Identify all nodes where ways should be split:
    // 1. Intersection nodes (degree >= 3)
    // 2. Highway type changes (degree = 2 but different highway types)
    // 3. Way endpoints (degree = 1)
    let mut junction_nodes = std::collections::HashSet::new();
    for (node_id, ways) in &node_to_ways {
        if ways.len() == 1 {
            // Way endpoint - always a junction
            junction_nodes.insert(*node_id);
        } else if ways.len() >= 3 {
            // True intersection - always a junction
            junction_nodes.insert(*node_id);
        } else if ways.len() == 2 {
            // Check for highway type changes or other properties that require splitting
            let way1 = &data.ways[&ways[0]];
            let way2 = &data.ways[&ways[1]];
            
            // Split if highway types are different
            if way1.tags.get("highway") != way2.tags.get("highway") {
                junction_nodes.insert(*node_id);
            }
            // Split if oneway properties differ
            else if way1.tags.get("oneway") != way2.tags.get("oneway") {
                junction_nodes.insert(*node_id);
            }
            // Split if one is a roundabout and the other isn't
            else if way1.tags.get("junction").is_some_and(|j| j == "roundabout") 
                != way2.tags.get("junction").is_some_and(|j| j == "roundabout") {
                junction_nodes.insert(*node_id);
            }
        }
    }

    for &node_id in &junction_nodes {
        if let Some(node) = data.nodes.get(&node_id) {
            let graph_node = IntersectionGraphNode {
                osm_node_id: node.id,
                point: geo::Point::new(node.lon, node.lat),
            };
            let index = graph.add_node(graph_node);
            osm_node_to_graph_index.insert(node.id, index);
        }
    }

    // Process each way and split it at junction nodes
    for way in data.ways.values() {
        if !is_road_way(way) {
            continue;
        }

        if way.nodes.is_empty() {
            continue;
        }

        // Find all junction nodes in this way
        let mut split_indices = Vec::new();
        for (i, &node_id) in way.nodes.iter().enumerate() {
            if junction_nodes.contains(&node_id) {
                split_indices.push(i);
            }
        }

        // If no junction nodes found, skip this way
        if split_indices.is_empty() {
            continue;
        }

        // Ensure the first and last nodes are included as split points
        if split_indices[0] != 0 {
            split_indices.insert(0, 0);
        }
        if split_indices[split_indices.len() - 1] != way.nodes.len() - 1 {
            split_indices.push(way.nodes.len() - 1);
        }

        // Create segments between consecutive junction nodes
        for i in 0..split_indices.len() - 1 {
            let start_idx = split_indices[i];
            let end_idx = split_indices[i + 1];
            
            if end_idx > start_idx {
                let segment_nodes = &way.nodes[start_idx..=end_idx];
                add_edge_from_path(
                    &mut graph,
                    &osm_node_to_graph_index,
                    segment_nodes,
                    way,
                    data,
                );
            }
        }
    }

    (graph, osm_node_to_graph_index)
}

/// Helper to create a RoadSegment and add it as an edge to the graph.
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
            return;
        }

        // Build geometry for the specific segment, not the entire way
        let segment_coords: Vec<geo::Coord> = path_nodes
            .iter()
            .filter_map(|&node_id| {
                data.nodes.get(&node_id).map(|node| geo::Coord {
                    x: node.lon,
                    y: node.lat,
                })
            })
            .collect();

        if segment_coords.len() >= 2 {
            let geometry = geo::LineString::from(segment_coords);
            let length_meters = Geodesic.length(&geometry);

            let segment = RoadSegment {
                osm_way_id: way.id,
                geometry,
                length_meters,
                highway_type: way.tags.get("highway").cloned(),
                name: way.tags.get("name").cloned(),
                start_node_id,
                end_node_id,
                intermediate_nodes: path_nodes.len().saturating_sub(2),
            };

            let oneway = way.tags.get("oneway").map(|s| s.as_str());
            let is_roundabout = way.tags.get("junction").is_some_and(|j| j == "roundabout");

            let forward = oneway == Some("yes")
                || oneway == Some("true")
                || oneway == Some("1")
                || is_roundabout;
            let backward = oneway == Some("-1");

            if !backward {
                graph.update_edge(start_index, end_index, segment.clone());
            }
            if !forward {
                graph.update_edge(end_index, start_index, segment);
            }
        }
    }
}

/// Get statistics about the road network graph
#[derive(Debug, Clone)]
pub struct GraphStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub total_length_km: f64,
    pub avg_segment_length_m: f64,
}

/// Calculate statistics for the road network graph
pub fn calculate_graph_stats(graph: &RoadGraph) -> GraphStats {
    let node_count = graph.node_count();
    let edge_count = graph.edge_count();
    
    let total_length_m: f64 = graph
        .edge_weights()
        .map(|segment| segment.length_meters)
        .sum();
    
    let total_length_km = total_length_m / 1000.0;
    let avg_segment_length_m = if edge_count > 0 {
        total_length_m / edge_count as f64
    } else {
        0.0
    };
    
    GraphStats {
        node_count,
        edge_count,
        total_length_km,
        avg_segment_length_m,
    }
}
