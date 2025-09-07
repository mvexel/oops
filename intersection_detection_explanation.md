# Intersection Detection Algorithm in `intersections.rs`

This document explains the intersection detection algorithm implemented in the `oops-analyzers/src/intersections.rs` file. The algorithm is designed to identify and analyze road intersections from OpenStreetMap (OSM) data.

## Overview

The core of the intersection detection algorithm is the `IntersectionAnalyzer`. This analyzer identifies logical intersections, including complex ones like roundabouts and multi-level interchanges, and provides detailed metadata for each. The analyzer doesn't just find simple nodes where ways cross; it uses a graph-based approach to identify and cluster related intersection nodes into "logical intersections."

The analysis process can be broken down into four main steps:

1.  **Build a road network graph:** The analyzer first constructs a graph representation of the road network from the OSM data.
2.  **Identify intersection nodes:** It then filters the nodes in the graph to find those that are likely to be part of an intersection.
3.  **Cluster intersection nodes:** The identified intersection nodes are then clustered based on their spatial proximity to form "logical intersections."
4.  **Extract metadata:** Finally, the analyzer extracts detailed metadata for each logical intersection.

## Detailed Breakdown

### 1. Building the Road Network Graph

The first step in the analysis is to build a road network graph. This is done in the `analyze` function using the `build_road_network` function from `oops_core::osm_analysis`.

```rust
// 1. Build the road network graph
let (graph, node_map) = build_road_network(data);
```

The `build_road_network` function (defined in `oops-core/src/osm_analysis.rs`) creates a `petgraph::Graph` where the nodes are intersections or dead-ends, and the edges are the road segments connecting them. This graph representation is essential for the subsequent steps of the analysis.

### 2. Identifying Intersection Nodes

Once the road network graph is built, the `analyze` function identifies the nodes that are likely to be part of an intersection. This is done by iterating through the nodes in the graph and filtering them based on their degree (the number of edges connected to them).

```rust
// 2. Filter graph nodes to those within the bounding box and degree >= 3 (actual intersections)
let mut intersection_nodes = Vec::new();
for (osm_node_id, &graph_idx) in &node_map {
    let node_weight = &graph[graph_idx];
    if rect.contains(&node_weight.point) {
        let degree = graph.edges(graph_idx).count();
        // Only include nodes with degree >= 3 (real intersections)
        if degree >= 3 {
            intersection_nodes.push(IntersectionNodePoint {
                node_id: *osm_node_id,
                point: node_weight.point,
                degree,
            });
        }
    }
}
```

In this step, the analyzer only considers nodes with a degree of 3 or more to be "real intersections." This is because nodes with a degree of 1 are typically dead-ends, and nodes with a degree of 2 are usually just points along a road (shaping points).

### 3. Clustering Intersection Nodes

After identifying the intersection nodes, the next step is to cluster them into "logical intersections." This is necessary because complex intersections, such as large roundabouts or interchanges, can be represented by multiple nodes in the OSM data. The clustering is done by the `cluster_intersection_nodes` function.

```rust
// 3. Apply spatial clustering to create logical intersections
let logical_intersections =
    cluster_intersection_nodes(&intersection_nodes, cluster_distance);
```

The `cluster_intersection_nodes` function uses a breadth-first search (BFS) algorithm to find connected components of intersection nodes. Two nodes are considered connected if the distance between them is less than or equal to the `max_distance` parameter. The function also has a special condition for clustering major intersections (degree >= 4), only clustering them if they are very close (within 35m). This prevents the algorithm from merging large, distinct intersections that happen to be close to each other.

### 4. Extracting Metadata

The final step in the analysis is to extract detailed metadata for each logical intersection. This is done by iterating through the `logical_intersections` and creating an `AnalysisResult` for each one.

```rust
// 4. Extract metadata for each logical intersection using the graph
let mut results = Vec::new();

for logical_intersection in logical_intersections {
    let mut result =
        AnalysisResult::new(logical_intersection.representative_node, ElementType::Node);
    let mut metadata = HashMap::new();

    // ... extract metadata ...

    result.add_finding(finding);
    results.push(result);
}
```

The analyzer extracts a variety of metadata for each logical intersection, including:

*   **Representative node:** The OSM node ID of a representative node for the intersection.
*   **Cluster size:** The number of intersection nodes in the cluster.
*   **Node cluster:** A list of all the OSM node IDs in the cluster.
*   **Cluster distance:** The maximum distance used for clustering.
*   **Coordinates:** The latitude and longitude of the representative node.
*   **Degree:** The number of ways connected to the intersection.
*   **Intersection type:** Whether the intersection is "simple" (a single node) or "complex" (multiple nodes).

This metadata is then used to create a `Finding` for each intersection, which is then added to the `AnalysisResult`.

## Conclusion

The intersection detection algorithm in `intersections.rs` is a sophisticated, graph-based approach to identifying and analyzing road intersections. By building a road network graph and clustering intersection nodes, the algorithm is able to identify both simple and complex intersections and provide detailed metadata for each. This information can be used for a variety of purposes, such as improving navigation systems, analyzing traffic flow, and identifying potential safety issues.
