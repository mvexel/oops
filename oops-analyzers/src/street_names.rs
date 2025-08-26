//! Missing Street Names Analyzer
//!
//! Tests for analyzing intersections where connected roads lack name tags

use oops_core::*;
use geo::*;
use geo::algorithm::haversine_distance::HaversineDistance;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_intersection_data() -> OsmData {
        let mut data = OsmData::new();

        // Create a proper 3-way intersection (T-junction)
        data.add_node(OsmNode::new(1, 0.0, 0.0)); // intersection center
        data.add_node(OsmNode::new(2, 0.001, 0.0)); // east
        data.add_node(OsmNode::new(3, -0.001, 0.0)); // west  
        data.add_node(OsmNode::new(4, 0.0, 0.001)); // north
        data.add_node(OsmNode::new(5, 0.0, -0.001)); // south

        // Way 1: East segment WITH name
        let mut way1 = OsmWay::new(1, vec![1, 2]);
        way1.tags.insert("highway".to_string(), "residential".to_string());
        way1.tags.insert("name".to_string(), "Main Street".to_string());
        data.ways.insert(1, way1);

        // Way 2: West segment WITH name  
        let mut way2 = OsmWay::new(2, vec![3, 1]);
        way2.tags.insert("highway".to_string(), "residential".to_string());
        way2.tags.insert("name".to_string(), "Main Street".to_string());
        data.ways.insert(2, way2);

        // Way 3: North road WITHOUT name (should be flagged)
        let mut way3 = OsmWay::new(3, vec![1, 4]);
        way3.tags.insert("highway".to_string(), "residential".to_string());
        // No name tag - this should trigger finding
        data.ways.insert(3, way3);

        data
    }

    fn create_named_intersection_data() -> OsmData {
        let mut data = OsmData::new();

        // Create intersection where ALL roads have names (3-way)
        data.add_node(OsmNode::new(10, 0.0, 0.0));
        data.add_node(OsmNode::new(11, 0.001, 0.0));
        data.add_node(OsmNode::new(12, -0.001, 0.0));
        data.add_node(OsmNode::new(13, 0.0, 0.001));

        // Way 1: East segment WITH name
        let mut way1 = OsmWay::new(10, vec![10, 11]);
        way1.tags.insert("highway".to_string(), "residential".to_string());
        way1.tags.insert("name".to_string(), "Oak Street".to_string());
        data.ways.insert(10, way1);

        // Way 2: West segment WITH name  
        let mut way2 = OsmWay::new(11, vec![12, 10]);
        way2.tags.insert("highway".to_string(), "residential".to_string());
        way2.tags.insert("name".to_string(), "Oak Street".to_string());
        data.ways.insert(11, way2);

        // Way 3: North segment WITH name
        let mut way3 = OsmWay::new(12, vec![10, 13]);
        way3.tags.insert("highway".to_string(), "residential".to_string());
        way3.tags.insert("name".to_string(), "Pine Avenue".to_string());
        data.ways.insert(12, way3);

        data
    }

    fn create_non_intersection_data() -> OsmData {
        let mut data = OsmData::new();

        // Create simple road segment (only 2 connected ways - not an intersection)
        data.add_node(OsmNode::new(20, 0.0, 0.0));
        data.add_node(OsmNode::new(21, 0.001, 0.0));
        data.add_node(OsmNode::new(22, 0.002, 0.0));

        // Single way connecting three nodes (no intersection)
        let mut way = OsmWay::new(20, vec![20, 21, 22]);
        way.tags.insert("highway".to_string(), "residential".to_string());
        // No name - but not an intersection so shouldn't be flagged
        data.ways.insert(20, way);

        data
    }

    #[test]
    fn test_missing_street_names_analyzer_creation() {
        let analyzer = MissingStreetNamesAnalyzer::new();
        assert_eq!(analyzer.name(), "missing_street_names");
        assert_eq!(analyzer.version(), "1.0.0");
        assert!(analyzer.description().contains("intersection"));
    }

    #[test]
    fn test_finds_missing_names_at_intersection() {
        let data = create_test_intersection_data();
        let analyzer = MissingStreetNamesAnalyzer::new();

        // Create query for the intersection area
        let bbox = BoundingBox::new(-0.002, -0.002, 0.002, 0.002);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        // Should find one result for the intersection
        assert_eq!(results.len(), 1, "Should find one intersection with missing names");

        let result = &results[0];
        assert_eq!(result.element_id, 1); // intersection node
        assert_eq!(result.element_type, ElementType::Node);
        assert_eq!(result.findings.len(), 1, "Should have one finding for unnamed road");

        let finding = &result.findings[0];
        assert_eq!(finding.category, ProblemCategory::Missing);
        assert_eq!(finding.severity, Severity::Medium);
        assert!(finding.message.contains("unnamed"));
        assert!(finding.message.contains("Intersection"));
    }

    #[test]
    fn test_no_findings_when_all_roads_named() {
        let data = create_named_intersection_data();
        let analyzer = MissingStreetNamesAnalyzer::new();

        let bbox = BoundingBox::new(-0.002, -0.002, 0.002, 0.002);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        // Should find the intersection but no problems
        assert_eq!(results.len(), 1, "Should find one intersection");
        let result = &results[0];
        assert_eq!(result.findings.len(), 0, "Should have no findings - all roads named");
    }

    #[test]
    fn test_ignores_non_intersections() {
        let data = create_non_intersection_data();
        let analyzer = MissingStreetNamesAnalyzer::new();

        let bbox = BoundingBox::new(-0.001, -0.001, 0.003, 0.001);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        // Should find no intersections (only degree-2 nodes exist)
        assert_eq!(results.len(), 0, "Should ignore non-intersection nodes");
    }

    #[test]
    fn test_spatial_distance_calculation() {
        // Test that we can find ways within distance of intersection
        let data = create_test_intersection_data();
        let intersection_point = Point::new(0.0, 0.0);

        // Test nearby way detection
        let way1_start = Point::new(0.001, 0.0); // 50m distance
        let way2_start = Point::new(0.0, 0.001); // 111m distance

        let distance1 = intersection_point.haversine_distance(&way1_start);
        let distance2 = intersection_point.haversine_distance(&way2_start);

        // Verify distances are reasonable (0.001 degrees ≈ 111m at equator)
        assert!(distance1 > 100.0 && distance1 < 120.0, "Distance should be ~111m, got {}", distance1);
        assert!(distance2 > 100.0 && distance2 < 120.0, "Distance should be ~111m, got {}", distance2);

        // Both should be within 200m search radius
        assert!(distance1 < 200.0);
        assert!(distance2 < 200.0);
    }

    #[test]
    fn test_analyzer_configuration() {
        let mut config = AnalyzerConfig::new("missing_street_names");
        config.set("search_radius_meters", "100.0");

        let analyzer = MissingStreetNamesAnalyzer::with_config(config);
        // Test that analyzer accepts configuration
        assert_eq!(analyzer.name(), "missing_street_names");
    }

    #[test]
    fn test_finding_context_includes_way_info() {
        let data = create_test_intersection_data();
        let analyzer = MissingStreetNamesAnalyzer::new();

        let bbox = BoundingBox::new(-0.002, -0.002, 0.002, 0.002);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);
        let finding = &results[0].findings[0];

        // Should include context about which way lacks name
        assert!(finding.context.contains_key("unnamed_way_id"));
        assert!(finding.context.contains_key("highway_type"));
        assert_eq!(finding.context.get("highway_type"), Some(&"residential".to_string()));
    }
}

// Analyzer implementation will go here after tests are written
pub struct MissingStreetNamesAnalyzer {
    search_radius_meters: f64,
}

impl MissingStreetNamesAnalyzer {
    pub fn new() -> Self {
        Self {
            search_radius_meters: 200.0, // Default search radius
        }
    }

    pub fn with_config(config: AnalyzerConfig) -> Self {
        let search_radius = config.get("search_radius_meters")
            .and_then(|s| s.parse().ok())
            .unwrap_or(200.0);

        Self {
            search_radius_meters: search_radius,
        }
    }
}

impl Analyzer for MissingStreetNamesAnalyzer {
    fn name(&self) -> &str {
        "missing_street_names"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        "Finds intersections where connected roads lack name tags, which can confuse navigation"
    }

    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();

        // Find all intersections in the data
        let intersection_nodes = find_road_intersections(data);

        for &intersection_id in &intersection_nodes {
            if let Some(intersection_node) = data.get_node(intersection_id) {
                let intersection_point = intersection_node.to_point();

                // Skip if intersection is outside query area
                if !query.area.contains_point(intersection_point.x(), intersection_point.y()) {
                    continue;
                }

                let mut result = AnalysisResult::new(intersection_id, ElementType::Node);
                
                // Find all ways connected to this intersection
                let connected_ways: Vec<_> = data.ways
                    .values()
                    .filter(|way| is_road_way(way) && way.nodes.contains(&intersection_id))
                    .collect();

                // Check each connected way for missing name
                for way in &connected_ways {
                    if !way.tags.contains_key("name") {
                        let mut context = std::collections::HashMap::new();
                        context.insert("unnamed_way_id".to_string(), way.id.to_string());
                        
                        if let Some(highway_type) = way.tags.get("highway") {
                            context.insert("highway_type".to_string(), highway_type.clone());
                        }

                        let finding = Finding {
                            category: ProblemCategory::Missing,
                            severity: Severity::Medium,
                            message: format!(
                                "Intersection has unnamed road (way {}). This can confuse navigation systems.",
                                way.id
                            ),
                            recommendation: Some("Add name tag to the road".to_string()),
                            context,
                        };

                        result.add_finding(finding);
                        break; // Only report once per intersection
                    }
                }

                // Only add result if we found issues OR if all roads are properly named
                // (for testing purposes, we want to know about good intersections too)
                results.push(result);
            }
        }

        results
    }
}