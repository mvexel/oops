pub mod analysis;
pub mod data;
pub mod geometry_simd;
pub mod graph;
pub mod intersections;
pub mod severity;
pub mod spatial;
pub mod spatial_index;
pub mod streaming;
pub mod tags;

pub use analysis::*;
pub use data::*;
pub use geometry_simd::*;
pub use graph::*;
pub use severity::*;
pub use spatial::*;
pub use spatial_index::*;
pub use streaming::*;
pub use tags::*;

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
        tags.insert("highway".into(), "crossing".into());

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
        context.insert("tag".into(), "amenity=restaurant".into());

        let finding = Finding {
            category: ProblemCategory::Missing,
            severity: Severity::Medium,
            message: Some("Opening hours not specified".into()),
            context,
        };

        assert_eq!(finding.severity, Severity::Medium);
        assert_eq!(finding.category, ProblemCategory::Missing);
        assert_eq!(
            finding.context.get("tag"),
            Some(&"amenity=restaurant".into())
        );
    }

    #[test]
    fn test_analysis_result_add_finding() {
        let mut result = AnalysisResult::new(456, ElementType::Way);

        let finding = Finding {
            category: ProblemCategory::Invalid,
            severity: Severity::High,
            message: Some("Invalid highway type".into()),
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
    fn test_composable_analyzer() {
        // Create a simple analyzer using the composable architecture
        let analyzer = Analyzer::new(
            "test_analyzer",
            "1.0.0",
            "Test analyzer for composable architecture",
            Box::new(WaySelector::new().highway_types(vec!["primary"])),
            Box::new(NoContext),
            Box::new(MissingTagDetector::new("maxspeed")),
        );

        let mut data = OsmData::new();

        // Add a way with highway=primary but no maxspeed
        let mut way_tags = HashMap::new();
        way_tags.insert("highway".into(), "primary".into());
        let way = OsmWay {
            id: 123,
            nodes: vec![1, 2],
            tags: way_tags,
        };
        data.ways.insert(way.id, way);

        // Add nodes for the way
        data.add_node(OsmNode::new(1, 13.4050, 52.5150));
        data.add_node(OsmNode::new(2, 13.4060, 52.5160));

        use geo::{Point, Rect};
        let bbox = BoundingBox::from_rect(Rect::new(
            Point::new(13.4000, 52.5100),
            Point::new(13.4100, 52.5200),
        ));

        let query = AnalysisQuery::new(bbox);
        let mut engine = AnalysisEngine::new();
        let results = engine.analyze_single(&analyzer, &query, &data);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].element_id, 123);
        assert_eq!(results[0].findings.len(), 1);
        assert_eq!(results[0].findings[0].category, ProblemCategory::Missing);
    }

    #[test]
    fn test_analyzer_config() {
        let mut config = AnalyzerConfig::new("crossing_analyzer");

        config.set("buffer_distance", "25.0");
        config.set("min_quality_level", "2");

        assert_eq!(config.name, "crossing_analyzer");
        assert_eq!(config.get("buffer_distance"), Some(&"25.0".into()));
        assert_eq!(config.get("min_quality_level"), Some(&"2".into()));
        assert_eq!(config.get("nonexistent"), None);
    }

    #[test]
    fn test_analyzer_registry() {
        let mut registry = AnalyzerRegistry::new();

        // Create test analyzers
        let simple_analyzer = Analyzer::new(
            "simple",
            "1.0.0",
            "A simple test analyzer",
            Box::new(WaySelector::new()),
            Box::new(NoContext),
            Box::new(MissingTagDetector::new("test_tag")),
        );

        let advanced_analyzer = Analyzer::new(
            "advanced",
            "2.1.0",
            "An advanced test analyzer",
            Box::new(WaySelector::new()),
            Box::new(NoContext),
            Box::new(MissingTagDetector::new("advanced_tag")),
        );

        // Test registration
        registry.register(simple_analyzer);
        registry.register(advanced_analyzer);

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
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.description, "A simple test analyzer");

        let info = registry.get_info("advanced").unwrap();
        assert_eq!(info.version, "2.1.0");

        // Test all metadata
        let all_info = registry.get_all_info();
        assert_eq!(all_info.len(), 2);

        // Test analysis
        let data = OsmData::new();
        let bbox = BoundingBox::new(0.0, 0.0, 1.0, 1.0);
        let query = AnalysisQuery::new(bbox);

        let result = registry.analyze("simple", &query, &data);
        assert!(result.is_ok());

        let result = registry.analyze("nonexistent", &query, &data);
        assert!(result.is_err());

        // Test analyze all
        let all_results = registry.analyze_all(&query, &data);
        assert_eq!(all_results.len(), 2);
        assert!(all_results.contains_key("simple"));
        assert!(all_results.contains_key("advanced"));
    }
}
