//! Graph representation of the OSM road network.
//!
//! This module provides functions to build a `petgraph` from OSM data,
//! where nodes are intersections and edges are road segments.

use crate::data::{OsmData, OsmWay};
use crate::intersections::is_road_way;
use crate::spatial::way_to_linestring;
use geo::GeodesicLength;
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
}

pub type RoadGraph = Graph<IntersectionGraphNode, RoadSegment, petgraph::Directed>;

/// Builds a graph representation of the road network from OSM data.
pub fn build_road_network(data: &OsmData) -> (RoadGraph, HashMap<i64, NodeIndex>) {
    let mut graph = RoadGraph::new();
    let mut osm_node_to_graph_index = HashMap::new();
    let node_to_ways = build_node_to_ways_index(data);

    let mut junction_nodes = std::collections::HashSet::new();
    for (node_id, ways) in &node_to_ways {
        if ways.len() != 2 {
            junction_nodes.insert(*node_id);
        } else {
            let way1 = &data.ways[&ways[0]];
            let way2 = &data.ways[&ways[1]];
            if way1.tags.get("highway") != way2.tags.get("highway") {
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

    for way in data.ways.values() {
        if !is_road_way(way) {
            continue;
        }

        let mut segment_start_index = 0;
        for i in 1..way.nodes.len() {
            let node_id = way.nodes[i];
            if junction_nodes.contains(&node_id) {
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

        if let Some(geometry) = way_to_linestring(way, data) {
            let length_meters = geometry.geodesic_length();

            let segment = RoadSegment {
                osm_way_id: way.id,
                geometry,
                length_meters,
                highway_type: way.tags.get("highway").cloned(),
                name: way.tags.get("name").cloned(),
            };

            let oneway = way.tags.get("oneway").map(|s| s.as_str());
            let is_roundabout = way.tags.get("junction").is_some_and(|j| j == "roundabout");

            let forward = oneway == Some("yes") || oneway == Some("true") || oneway == Some("1") || is_roundabout;
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

/// Build a fast lookup index from nodes to connected ways.
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

