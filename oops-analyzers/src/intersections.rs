//! Intersection Analyzer
//!
//! Extracts detailed metadata about all logical intersections in the analyzed area.
//! This analyzer doesn't identify problems - instead it provides comprehensive
//! intersection data that can be used by other analyzers or for visualization.

use geo::{Contains, HaversineDistance, Rect};
use oops_core::*;
use std::collections::HashMap;

/// Analyzer that extracts metadata for all intersections in an area
///
/// This analyzer identifies logical intersections (including complex ones like
/// divided highway intersections) and outputs comprehensive metadata for each.
/// The results contain no "findings" - just rich intersection data in the context.
pub struct IntersectionAnalyzer {
    /// Configuration for the analyzer behavior
    config: AnalyzerConfig,
}

impl IntersectionAnalyzer {
    /// Create a new analyzer with default settings
    pub fn new() -> Self {
        let mut config = AnalyzerConfig::new("intersections");
        config.set("cluster_distance", "30.0"); // Default 30m clustering - allow clustering of closely spaced major intersections
        Self { config }
    }

    /// Create a new analyzer with custom configuration
    pub fn with_config(config: AnalyzerConfig) -> Self {
        Self { config }
    }

    /// Create a new analyzer in validation mode for testing against ground truth
    pub fn validation_mode() -> Self {
        let mut config = AnalyzerConfig::new("intersections");
        config.set("cluster_distance", "30.0");
        config.set("validation_mode", "true");
        Self { config }
    }

    /// Get the clustering distance from configuration
    fn get_cluster_distance(&self) -> f64 {
        self.config
            .get("cluster_distance")
            .and_then(|s| s.parse().ok())
            .unwrap_or(50.0) // Default 50m
    }

    /// Convert BoundingBox to geo::Rect for spatial functions
    fn bbox_to_rect(bbox: &BoundingBox) -> Rect {
        Rect::new(
            geo::Coord {
                x: bbox.min_lon,
                y: bbox.min_lat,
            },
            geo::Coord {
                x: bbox.max_lon,
                y: bbox.max_lat,
            },
        )
    }
}

impl Analyzer for IntersectionAnalyzer {
    /// Return the unique identifier for this analyzer
    fn name(&self) -> &str {
        "intersections"
    }

    /// Return the version of this analyzer
    fn version(&self) -> &str {
        "2.0.0" // Version bump to reflect architectural change
    }

    /// Return a human-readable description of what this analyzer does
    fn description(&self) -> &str {
        "Extracts comprehensive metadata for all road intersections using a graph-based network model"
    }

    /// Analyze OSM data to extract logical intersection metadata using graph-based clustering.
    ///
    /// This analyzer builds a road network graph and then applies spatial clustering to
    /// nearby intersection nodes to identify logical intersections. Complex intersections
    /// (multiple clustered nodes) and simple intersections (single nodes) are identified.
    ///
    /// # Arguments
    /// * `query` - Defines the area for analysis
    /// * `data` - OSM data to analyze (nodes, ways, relations)
    ///
    /// # Returns
    /// Vector of analysis results, one per logical intersection found in the query area.
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let rect = Self::bbox_to_rect(&query.area);
        let cluster_distance = self.get_cluster_distance();

        // 1. Build the road network graph
        let (graph, node_map) = build_road_network(data);

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

        if intersection_nodes.is_empty() {
            return Vec::new();
        }

        // 3. Apply spatial clustering to create logical intersections
        let logical_intersections =
            cluster_intersection_nodes(&intersection_nodes, cluster_distance);

        // 4. Extract metadata for each logical intersection using the graph
        let mut results = Vec::new();

        // Store logical_intersections for validation before consuming it
        let logical_intersections_for_validation = logical_intersections.clone();

        for logical_intersection in logical_intersections {
            let mut result =
                AnalysisResult::new(logical_intersection.representative_node, ElementType::Node);
            let mut metadata = HashMap::new();

            // Basic cluster information
            let cluster_size = logical_intersection.node_cluster.len();
            metadata.insert(
                "representative_node".to_string(),
                logical_intersection.representative_node.to_string(),
            );
            metadata.insert("cluster_size".to_string(), cluster_size.to_string());
            metadata.insert(
                "node_cluster".to_string(),
                logical_intersection
                    .node_cluster
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
            metadata.insert("cluster_distance".to_string(), cluster_distance.to_string());

            // Get coordinates of representative node
            if let Some(node) = data.nodes.get(&logical_intersection.representative_node) {
                metadata.insert(
                    "coordinates".to_string(),
                    format!("{},{}", node.lat, node.lon),
                );
            }

            // Collect all unique ways connected to any node in the cluster
            let mut connected_ways: std::collections::HashSet<i64> =
                std::collections::HashSet::new();
            for &node_id in &logical_intersection.node_cluster {
                if let Some(&graph_idx) = node_map.get(&node_id) {
                    // Collect way IDs from graph edges (both directions)
                    for edge_ref in graph.edges_directed(graph_idx, Direction::Outgoing) {
                        connected_ways.insert(edge_ref.weight().osm_way_id);
                    }
                    for edge_ref in graph.edges_directed(graph_idx, Direction::Incoming) {
                        connected_ways.insert(edge_ref.weight().osm_way_id);
                    }
                }
            }

            let degree = connected_ways.len();
            metadata.insert("degree".to_string(), degree.to_string());

            // For simple intersections (cluster_size=1), a degree of 2 or less is just a
            // continuation of a road or a dead-end, not a true intersection point.
            if cluster_size == 1 && degree < 3 {
                continue;
            }

            // Intersection type classification
            let intersection_type = if cluster_size > 1 {
                "complex" // Multiple nodes = complex intersection
            } else {
                "simple" // Single node = simple intersection
            };
            metadata.insert(
                "intersection_type".to_string(),
                intersection_type.to_string(),
            );

            // Create informational finding
            let message = if cluster_size > 1 {
                format!(
                    "Complex logical intersection with {} clustered nodes and {} connected ways",
                    cluster_size, degree
                )
            } else {
                format!("Simple intersection with {} connected ways", degree)
            };

            let finding = Finding {
                category: ProblemCategory::Info,
                severity: Severity::Info,
                message: Some(message),
                recommendation: None,
                context: metadata,
            };

            result.add_finding(finding);
            results.push(result);
        }

        // If in validation mode, add validation results
        if self.config.get("validation_mode").is_some() {
            IntersectionAnalyzer::validate_against_ground_truth(
                &logical_intersections_for_validation,
                data,
                &mut results,
            );
        }

        results
    }
}

impl IntersectionAnalyzer {
    /// Validate algorithm results against ground truth tags in OSM data
    fn validate_against_ground_truth(
        logical_intersections: &[LogicalIntersection],
        data: &OsmData,
        results: &mut Vec<AnalysisResult>,
    ) {
        let mut validation_summary = HashMap::new();
        let mut correct_clusters = 0;
        let mut incorrect_clusters = 0;
        let mut missing_expected = 0;

        // Check each logical intersection against ground truth
        for intersection in logical_intersections {
            if let Some(node) = data.nodes.get(&intersection.representative_node) {
                let mut validation_result =
                    AnalysisResult::new(intersection.representative_node, ElementType::Node);
                let mut validation_context = HashMap::new();

                // Check expected cluster size
                if let Some(expected_size_str) = node.tags.get("expected_cluster_size") {
                    if let Ok(expected_size) = expected_size_str.parse::<usize>() {
                        let actual_size = intersection.node_cluster.len();
                        let size_correct = actual_size == expected_size;

                        validation_context.insert(
                            "expected_cluster_size".to_string(),
                            expected_size.to_string(),
                        );
                        validation_context
                            .insert("actual_cluster_size".to_string(), actual_size.to_string());
                        validation_context
                            .insert("cluster_size_correct".to_string(), size_correct.to_string());

                        if size_correct {
                            correct_clusters += 1;
                        } else {
                            incorrect_clusters += 1;
                        }
                    }
                }

                // Check expected degree
                if let Some(expected_degree_str) = node.tags.get("expected_degree") {
                    if let Ok(expected_degree) = expected_degree_str.parse::<usize>() {
                        // Calculate actual degree for this intersection
                        let mut connected_ways: std::collections::HashSet<i64> =
                            std::collections::HashSet::new();
                        for &node_id in &intersection.node_cluster {
                            for way in data.ways.values() {
                                if way.tags.contains_key("highway") && way.nodes.contains(&node_id)
                                {
                                    connected_ways.insert(way.id);
                                }
                            }
                        }
                        let actual_degree = connected_ways.len();
                        let degree_correct = actual_degree == expected_degree;

                        validation_context
                            .insert("expected_degree".to_string(), expected_degree.to_string());
                        validation_context
                            .insert("actual_degree".to_string(), actual_degree.to_string());
                        validation_context
                            .insert("degree_correct".to_string(), degree_correct.to_string());
                    }
                }

                // Check expected intersection type
                if let Some(expected_type) = node.tags.get("expected_type") {
                    let actual_type = if intersection.node_cluster.len() > 1 {
                        "complex"
                    } else {
                        "simple"
                    };
                    let type_correct = actual_type == expected_type;

                    validation_context.insert("expected_type".to_string(), expected_type.clone());
                    validation_context.insert("actual_type".to_string(), actual_type.to_string());
                    validation_context.insert("type_correct".to_string(), type_correct.to_string());
                }

                // Check cluster_with relationships
                if let Some(cluster_with_str) = node.tags.get("cluster_with") {
                    let expected_cluster_nodes: Vec<i64> = cluster_with_str
                        .split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect();

                    let mut cluster_relationships_correct = true;
                    for expected_node in &expected_cluster_nodes {
                        if !intersection.node_cluster.contains(expected_node) {
                            cluster_relationships_correct = false;
                            break;
                        }
                    }

                    validation_context.insert(
                        "expected_cluster_nodes".to_string(),
                        cluster_with_str.clone(),
                    );
                    validation_context.insert(
                        "actual_cluster_nodes".to_string(),
                        intersection
                            .node_cluster
                            .iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    validation_context.insert(
                        "cluster_relationships_correct".to_string(),
                        cluster_relationships_correct.to_string(),
                    );
                }

                // Only add validation result if there were ground truth tags
                if !validation_context.is_empty() {
                    let finding = Finding {
                        category: ProblemCategory::Info,
                        severity: Severity::Info,
                        message: Some(format!(
                            "Validation result for intersection {}",
                            intersection.representative_node
                        )),
                        recommendation: None,
                        context: validation_context,
                    };
                    validation_result.add_finding(finding);
                    results.push(validation_result);
                }
            }
        }

        // Check for expected intersections that were missed
        for (node_id, node) in &data.nodes {
            if node.tags.contains_key("expected_cluster_size")
                || node.tags.contains_key("expected_degree")
                || node.tags.contains_key("expected_type")
            {
                // Check if this node appears in any of our detected intersections
                let found_in_results = logical_intersections
                    .iter()
                    .any(|intersection| intersection.node_cluster.contains(node_id));

                if !found_in_results {
                    missing_expected += 1;
                    let mut missed_result = AnalysisResult::new(*node_id, ElementType::Node);
                    let finding = Finding {
                        category: ProblemCategory::Missing,
                        severity: Severity::Medium,
                        message: Some(format!(
                            "Expected intersection {} was not detected by algorithm",
                            node_id
                        )),
                        recommendation: Some(
                            "Check clustering parameters or algorithm logic".to_string(),
                        ),
                        context: HashMap::new(),
                    };
                    missed_result.add_finding(finding);
                    results.push(missed_result);
                }
            }
        }

        // Add overall validation summary
        validation_summary.insert("correct_clusters".to_string(), correct_clusters.to_string());
        validation_summary.insert(
            "incorrect_clusters".to_string(),
            incorrect_clusters.to_string(),
        );
        validation_summary.insert("missing_expected".to_string(), missing_expected.to_string());
        validation_summary.insert(
            "total_detected".to_string(),
            logical_intersections.len().to_string(),
        );

        let mut summary_result = AnalysisResult::new(0, ElementType::Node); // Use 0 as summary ID
        let summary_finding = Finding {
            category: ProblemCategory::Info,
            severity: Severity::Info,
            message: Some("Validation Summary".to_string()),
            recommendation: None,
            context: validation_summary,
        };
        summary_result.add_finding(summary_finding);
        results.push(summary_result);
    }
}

/// Clusters intersection nodes spatially into logical intersections
///
/// Uses a breadth-first search approach to find connected components of nodes
/// within the given `max_distance`. This is more robust than the previous
/// implementation as it correctly handles transitive relationships.
fn cluster_intersection_nodes(
    nodes: &[IntersectionNodePoint],
    max_distance: f64,
) -> Vec<LogicalIntersection> {
    if nodes.is_empty() {
        return Vec::new();
    }

    let mut visited = std::collections::HashSet::new();
    let mut clusters = Vec::new();

    for i in 0..nodes.len() {
        if visited.contains(&nodes[i].node_id) {
            continue;
        }

        let mut current_cluster = Vec::new();
        let mut queue = std::collections::VecDeque::new();

        queue.push_back(i);
        visited.insert(nodes[i].node_id);

        while let Some(current_idx) = queue.pop_front() {
            let current_node = &nodes[current_idx];
            current_cluster.push(current_node.node_id);

            // Find all unvisited nodes within max_distance of the current node
            for j in 0..nodes.len() {
                let neighbor_node = &nodes[j];
                if !visited.contains(&neighbor_node.node_id) {
                    let distance = current_node.point.haversine_distance(&neighbor_node.point);

                    if distance <= max_distance {
                        // Allow clustering if:
                        // 1. The neighbor is not a major intersection (degree < 4), OR
                        // 2. The neighbor is a major intersection but very close (within 35m)
                        if neighbor_node.degree < 4 || distance <= 35.0 {
                            visited.insert(neighbor_node.node_id);
                            queue.push_back(j);
                        }
                    }
                }
            }
        }
        clusters.push(current_cluster);
    }

    // Convert clusters to LogicalIntersection structs
    clusters
        .into_iter()
        .map(|mut cluster| {
            cluster.sort(); // Consistent ordering
            LogicalIntersection {
                representative_node: cluster[0], // Use first node as representative
                node_cluster: cluster,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A more robust test case for the graph analysis
    fn create_test_data() -> OsmData {
        let mut data = OsmData::new();

        // J1 (T-junction, osm_id: 1)
        data.add_node(OsmNode::new(1, 0.0, 0.0));
        data.add_node(OsmNode::new(10, -0.001, 0.0)); // West
        data.add_node(OsmNode::new(11, 0.001, 0.0)); // East
        data.add_node(OsmNode::new(12, 0.0, 0.001)); // North

        // Dead end D1
        data.add_node(OsmNode::new(20, -0.002, 0.0));

        // Way from D1 to J1
        let mut way1 = OsmWay::new(101, vec![20, 10, 1]);
        way1.tags.insert("highway".into(), "residential".into());
        data.ways.insert(101, way1);

        // Way from J1 continuing east
        let mut way2 = OsmWay::new(102, vec![1, 11]);
        way2.tags.insert("highway".into(), "residential".into());
        data.ways.insert(102, way2);

        // Way from J1 north
        let mut way3 = OsmWay::new(103, vec![1, 12]);
        way3.tags.insert("highway".into(), "secondary".into());
        data.ways.insert(103, way3);

        // J2 (4-way intersection, osm_id: 2)
        data.add_node(OsmNode::new(2, 0.1, 0.1));
        data.add_node(OsmNode::new(200, 0.1, 0.101));
        data.add_node(OsmNode::new(201, 0.1, 0.099));
        data.add_node(OsmNode::new(202, 0.101, 0.1));
        data.add_node(OsmNode::new(203, 0.099, 0.1));

        let mut way20 = OsmWay::new(201, vec![200, 2, 201]);
        way20.tags.insert("highway".into(), "primary".into());
        data.ways.insert(201, way20);
        let mut way21 = OsmWay::new(202, vec![202, 2, 203]);
        way21.tags.insert("highway".into(), "primary".into());
        data.ways.insert(202, way21);

        data
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = IntersectionAnalyzer::new();
        assert_eq!(analyzer.name(), "intersections");
        assert!(analyzer.description().contains("graph-based"));
    }

    #[test]
    fn test_finds_intersections_graph_based() {
        let data = create_test_data();
        let analyzer = IntersectionAnalyzer::new();

        let bbox = BoundingBox::new(-0.01, -0.01, 0.01, 0.01);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);
        // Junctions in bbox: 1 (degree 3), 11 (degree 1), 12 (degree 1), 20 (degree 1)
        // Node 10 is a shaping node (degree 2) and should not be in the graph.
        assert_eq!(
            results.len(),
            4,
            "Should find 4 junctions (1 intersection, 3 dead-ends)"
        );

        // Find the result for the main intersection (node 1)
        let intersection_result = results.iter().find(|r| r.element_id == 1).unwrap();
        let context = &intersection_result.findings[0].context;
        assert_eq!(context.get("degree"), Some(&"3".to_string()));
        assert_eq!(context.get("complexity"), Some(&"simple".to_string()));

        // Find the result for the dead-end at node 20
        let dead_end_result = results.iter().find(|r| r.element_id == 20).unwrap();
        let context = &dead_end_result.findings[0].context;
        assert_eq!(context.get("degree"), Some(&"1".to_string()));
    }
}
