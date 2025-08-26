pub mod analysis;
pub mod data;
pub mod osm_analysis;

pub use analysis::*;
pub use data::*;
pub use osm_analysis::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_osm_node_creation() {
        let node = OsmNode {
            id: 123,
            lat: 52.5200,
            lon: 13.4050,
            tags: HashMap::new(),
        };

        assert_eq!(node.id, 123);
        assert_eq!(node.lat, 52.5200);
        assert_eq!(node.lon, 13.4050);
        assert!(node.tags.is_empty());
    }

    #[test]
    fn test_osm_node_to_point() {
        let mut tags = HashMap::new();
        tags.insert("highway".to_string(), "crossing".to_string());

        let node = OsmNode {
            id: 456,
            lat: 40.7128,
            lon: -74.0060,
            tags,
        };

        let point = node.to_point();
        assert_eq!(point.x(), -74.0060);
        assert_eq!(point.y(), 40.7128);
    }

    #[test]
    fn test_osm_way_creation() {
        let way = OsmWay {
            id: 789,
            nodes: vec![1, 2, 3, 4],
            tags: HashMap::new(),
        };

        assert_eq!(way.id, 789);
        assert_eq!(way.nodes.len(), 4);
        assert_eq!(way.nodes[0], 1);
    }

    #[test]
    fn test_osm_way_is_closed() {
        let closed_way = OsmWay {
            id: 100,
            nodes: vec![1, 2, 3, 1], // First and last are same
            tags: HashMap::new(),
        };

        let open_way = OsmWay {
            id: 101,
            nodes: vec![1, 2, 3, 4],
            tags: HashMap::new(),
        };

        assert!(closed_way.is_closed());
        assert!(!open_way.is_closed());
    }

    #[test]
    fn test_osm_data_creation() {
        let data = OsmData::new();
        assert!(data.nodes.is_empty());
        assert!(data.ways.is_empty());
        assert!(data.relations.is_empty());
    }

    #[test]
    fn test_osm_data_add_node() {
        let mut data = OsmData::new();
        let node = OsmNode::new(123, 13.4050, 52.5200);

        data.add_node(node.clone());

        assert_eq!(data.nodes.len(), 1);
        assert_eq!(data.get_node(123), Some(&node));
    }

    #[test]
    fn test_osm_data_spatial_query() {
        let mut data = OsmData::new();

        // Add nodes in Berlin area
        data.add_node(OsmNode::new(1, 13.4050, 52.5200)); // Berlin center
        data.add_node(OsmNode::new(2, 13.4060, 52.5210)); // Very close - ~150m
        data.add_node(OsmNode::new(3, -74.0060, 40.7128)); // NYC - far away

        // Query small area around Berlin center
        let nearby_nodes = data.nodes_near_point(52.5200, 13.4050, 1000.0); // 1km radius

        assert_eq!(nearby_nodes.len(), 2); // Should find nodes 1 and 2, not 3
        assert!(nearby_nodes.iter().any(|n| n.id == 1));
        assert!(nearby_nodes.iter().any(|n| n.id == 2));
        assert!(!nearby_nodes.iter().any(|n| n.id == 3));
    }

    #[test]
    fn test_analysis_result_creation() {
        let result = AnalysisResult::new(123, ElementType::Node);

        assert_eq!(result.element_id, 123);
        assert_eq!(result.element_type, ElementType::Node);
        assert!(result.findings.is_empty());
        assert_eq!(result.overall_severity, Severity::Info);
        assert_eq!(result.confidence, 1.0);
    }

    #[test]
    fn test_finding_creation() {
        let mut context = HashMap::new();
        context.insert("tag".to_string(), "amenity=restaurant".to_string());

        let finding = Finding {
            category: ProblemCategory::Missing,
            severity: Severity::Medium,
            message: "Opening hours not specified".to_string(),
            recommendation: Some("Add opening_hours tag".to_string()),
            context,
        };

        assert_eq!(finding.severity, Severity::Medium);
        assert_eq!(finding.category, ProblemCategory::Missing);
        assert!(finding.recommendation.is_some());
        assert_eq!(
            finding.context.get("tag"),
            Some(&"amenity=restaurant".to_string())
        );
    }

    #[test]
    fn test_analysis_result_add_finding() {
        let mut result = AnalysisResult::new(456, ElementType::Way);

        let finding = Finding {
            category: ProblemCategory::Invalid,
            severity: Severity::High,
            message: "Invalid highway type".to_string(),
            recommendation: None,
            context: HashMap::new(),
        };

        result.add_finding(finding);

        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.overall_severity, Severity::High); // Should update to highest severity
    }

    #[test]
    fn test_analysis_query_creation() {
        use geo::{Point, Rect};

        let bbox = BoundingBox::from_rect(Rect::new(
            Point::new(13.4000, 52.5100),
            Point::new(13.4100, 52.5200),
        ));

        let query = AnalysisQuery::new(bbox)
            .with_element(ElementRef::Node(123))
            .with_element(ElementRef::Intersection(456));

        assert_eq!(query.elements.len(), 2);
        assert!(matches!(query.elements[0], ElementRef::Node(123)));
        assert!(matches!(query.elements[1], ElementRef::Intersection(456)));
    }

    #[test]
    fn test_analyzer_trait() {
        struct TestAnalyzer;

        impl Analyzer for TestAnalyzer {
            fn name(&self) -> &str {
                "poi_analyzer"
            }

            fn description(&self) -> &str {
                "Analyzes POI data quality"
            }

            fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
                let mut results = Vec::new();

                // Analyze all nodes in the query area
                for node in data.nodes_in_bbox(&query.area) {
                    if node.tags.contains_key("amenity") {
                        let mut result = AnalysisResult::new(node.id, ElementType::Node);

                        if !node.tags.contains_key("opening_hours") {
                            let finding = Finding {
                                category: ProblemCategory::Missing,
                                severity: Severity::Medium,
                                message: "Missing opening hours".to_string(),
                                recommendation: Some("Add opening_hours tag".to_string()),
                                context: HashMap::new(),
                            };
                            result.add_finding(finding);
                        }

                        results.push(result);
                    }
                }

                results
            }
        }

        let analyzer = TestAnalyzer;
        let mut data = OsmData::new();

        // Add POI node with amenity
        let mut poi_tags = HashMap::new();
        poi_tags.insert("amenity".to_string(), "restaurant".to_string());
        let poi_node = OsmNode {
            id: 123,
            lon: 13.4050,
            lat: 52.5150,
            tags: poi_tags,
        };
        data.add_node(poi_node);

        // Add regular node (no amenity)
        data.add_node(OsmNode::new(456, 13.4060, 52.5160));

        use geo::{Point, Rect};
        let bbox = BoundingBox::from_rect(Rect::new(
            Point::new(13.4000, 52.5100),
            Point::new(13.4100, 52.5200),
        ));

        let query = AnalysisQuery::new(bbox);
        let results = analyzer.analyze(&query, &data);

        assert_eq!(results.len(), 1); // Only POI node should be analyzed
        assert_eq!(results[0].element_id, 123);
        assert_eq!(results[0].findings.len(), 1);
        assert_eq!(results[0].findings[0].category, ProblemCategory::Missing);
    }

    #[test]
    fn test_intersection_analyzer() {
        struct IntersectionAnalyzer;

        impl Analyzer for IntersectionAnalyzer {
            fn name(&self) -> &str {
                "intersection_analyzer"
            }

            fn description(&self) -> &str {
                "Analyzes intersection safety and accessibility"
            }

            fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
                let mut results = Vec::new();

                // Find intersection nodes (simplified - would normally check way connectivity)
                for element in &query.elements {
                    if let ElementRef::Intersection(node_id) = element {
                        if let Some(node) = data.get_node(*node_id) {
                            let mut result = AnalysisResult::new(*node_id, ElementType::Node);

                            // Look for nearby sidewalks within buffer
                            let nearby_nodes = data.nodes_near_point(
                                node.lat,
                                node.lon,
                                query.context.buffer_distance,
                            );

                            let sidewalk_count = nearby_nodes
                                .iter()
                                .filter(|n| {
                                    n.tags.get("highway") == Some(&"footway".to_string())
                                        || n.tags.get("footway") == Some(&"sidewalk".to_string())
                                })
                                .count();

                            if sidewalk_count == 0 {
                                let finding = Finding {
                                    category: ProblemCategory::Missing,
                                    severity: Severity::Critical,
                                    message: "No sidewalks found near intersection".to_string(),
                                    recommendation: Some(
                                        "Add mapped sidewalks or footway=sidewalk tags".to_string(),
                                    ),
                                    context: HashMap::new(),
                                };
                                result.add_finding(finding);
                            }

                            results.push(result);
                        }
                    }
                }

                results
            }
        }

        let analyzer = IntersectionAnalyzer;
        let mut data = OsmData::new();

        // Add intersection node
        data.add_node(OsmNode::new(999, 13.4050, 52.5150));

        // Add a sidewalk node nearby (within buffer)
        let mut sidewalk_tags = HashMap::new();
        sidewalk_tags.insert("highway".to_string(), "footway".to_string());
        sidewalk_tags.insert("footway".to_string(), "sidewalk".to_string());
        data.add_node(OsmNode {
            id: 1000,
            lon: 13.4051,
            lat: 52.5151, // Close to intersection
            tags: sidewalk_tags,
        });

        use geo::{Point, Rect};
        let bbox = BoundingBox::from_rect(Rect::new(
            Point::new(13.4000, 52.5100),
            Point::new(13.4100, 52.5200),
        ));

        let query = AnalysisQuery::new(bbox).with_element(ElementRef::Intersection(999));

        let results = analyzer.analyze(&query, &data);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].element_id, 999);
        // Should find sidewalk nearby, so no critical findings
        assert_eq!(results[0].findings.len(), 0);
    }

    #[test]
    fn test_analyzer_config() {
        let mut config = AnalyzerConfig::new("crossing_analyzer");

        config.set("buffer_distance", "25.0");
        config.set("min_quality_level", "2");

        assert_eq!(config.name, "crossing_analyzer");
        assert_eq!(config.get("buffer_distance"), Some(&"25.0".to_string()));
        assert_eq!(config.get("min_quality_level"), Some(&"2".to_string()));
        assert_eq!(config.get("nonexistent"), None);
    }

    #[test]
    fn test_analyzer_registry() {
        let mut registry = AnalyzerRegistry::new();

        struct SimpleAnalyzer;
        impl Analyzer for SimpleAnalyzer {
            fn name(&self) -> &str {
                "simple"
            }
            fn description(&self) -> &str {
                "A simple test analyzer"
            }
            fn analyze(&self, _query: &AnalysisQuery, _data: &OsmData) -> Vec<AnalysisResult> {
                vec![AnalysisResult::new(123, ElementType::Node)]
            }
        }

        struct AdvancedAnalyzer;
        impl Analyzer for AdvancedAnalyzer {
            fn name(&self) -> &str {
                "advanced"
            }
            fn version(&self) -> &str {
                "2.1.0"
            }
            fn description(&self) -> &str {
                "An advanced test analyzer"
            }
            fn analyze(&self, _query: &AnalysisQuery, _data: &OsmData) -> Vec<AnalysisResult> {
                Vec::new()
            }
        }

        // Test registration
        registry.register(SimpleAnalyzer);
        registry.register(AdvancedAnalyzer);

        // Test listing
        let analyzers = registry.list();
        assert_eq!(analyzers.len(), 2);
        assert!(analyzers.contains(&"simple"));
        assert!(analyzers.contains(&"advanced"));

        // Test retrieval
        assert!(registry.get("simple").is_some());
        assert!(registry.get("nonexistent").is_none());

        // Test metadata
        let info = registry.get_info("simple").unwrap();
        assert_eq!(info.name, "simple");
        assert_eq!(info.version, "1.0.0"); // default version
        assert_eq!(info.description, "A simple test analyzer");

        let info = registry.get_info("advanced").unwrap();
        assert_eq!(info.version, "2.1.0"); // custom version

        // Test all metadata
        let all_info = registry.get_all_info();
        assert_eq!(all_info.len(), 2);

        // Test analysis
        let data = OsmData::new();
        let bbox = BoundingBox::new(0.0, 0.0, 1.0, 1.0);
        let query = AnalysisQuery::new(bbox);

        let result = registry.analyze("simple", &query, &data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);

        let result = registry.analyze("nonexistent", &query, &data);
        assert!(result.is_err());

        // Test analyze all
        let all_results = registry.analyze_all(&query, &data);
        assert_eq!(all_results.len(), 2);
        assert!(all_results.contains_key("simple"));
        assert!(all_results.contains_key("advanced"));
    }
}
