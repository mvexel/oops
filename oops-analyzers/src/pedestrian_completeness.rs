use oops_core::*;
use oops_core::calculate_sidewalk_coverage;
use rstar::RTree;
use std::collections::HashMap;

/// Analyzer that identifies roads missing sidewalk tags.
pub struct PedestrianCompletenessAnalyzer {
    config: AnalyzerConfig,
}

impl Default for PedestrianCompletenessAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PedestrianCompletenessAnalyzer {
    /// Create a new analyzer with default settings.
    pub fn new() -> Self {
        let mut config = AnalyzerConfig::new("pedestrian_completeness");
        // Highway-specific distance thresholds (meters)
        config.set("distance_residential", "20.0");
        config.set("distance_unclassified", "20.0"); 
        config.set("distance_tertiary", "25.0");
        config.set("distance_secondary", "30.0");
        config.set("distance_primary", "35.0");
        config.set("max_angle_diff", "30.0");
        config.set("min_coverage_ratio", "0.5");
        Self { config }
    }

    /// Create a new analyzer with custom configuration.
    pub fn with_config(config: AnalyzerConfig) -> Self {
        Self { config }
    }

    /// Get the distance threshold for a specific highway type.
    fn get_distance_threshold(&self, highway_type: &str) -> f64 {
        let config_key = format!("distance_{}", highway_type);
        self.config.get(&config_key)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(20.0) // Fallback to 20m for unknown highway types
    }

    /// Check if a road way should be analyzed for sidewalk completeness.
    fn should_analyze_way(&self, way: &OsmWay) -> bool {
        let highway_type = match way.tags.get("highway") {
            Some(highway) => highway,
            None => return false, // Not a road
        };

        let relevant_road_types = [
            "primary",
            "secondary",
            "tertiary",
            "residential",
            "unclassified",
        ];

        relevant_road_types.contains(&highway_type.as_str())
    }

    /// Check if a way has a sidewalk tag.
    fn has_sidewalk_tag(&self, way: &OsmWay) -> bool {
        // Check for generic sidewalk tag
        if way.tags.contains_key("sidewalk") {
            return true;
        }
        
        // Check for specific sidewalk:left and sidewalk:right tags
        way.tags.contains_key("sidewalk:left") || way.tags.contains_key("sidewalk:right")
    }

    fn create_finding_context(&self, way: &OsmWay, nearby_sidewalks: &[&WayWithLineString], coverage_ratio: f64) -> HashMap<String, String> {
        let mut context = HashMap::new();
        context.insert("way_id".into(), way.id.to_string());
        if let Some(highway_type) = way.tags.get("highway") {
            context.insert("highway_type".into(), highway_type.clone());
        }
        if let Some(name) = way.tags.get("name") {
            context.insert("way_name".into(), name.clone());
        }
        
        // Add debug information
        context.insert("nearby_sidewalk_count".into(), nearby_sidewalks.len().to_string());
        context.insert("coverage_ratio".into(), format!("{:.3}", coverage_ratio));
        context.insert("coverage_percent".into(), format!("{:.1}", coverage_ratio * 100.0));
        
        if !nearby_sidewalks.is_empty() {
            let sidewalk_ids: Vec<String> = nearby_sidewalks.iter()
                .map(|sw| sw.way.id.to_string())
                .collect();
            context.insert("nearby_sidewalk_ids".into(), sidewalk_ids.join(","));
        }
        
        // Add configuration info
        let highway_type = way.tags.get("highway").map(|s| s.as_str()).unwrap_or("unknown");
        let max_distance = self.get_distance_threshold(highway_type);
        let min_coverage = self.config.get("min_coverage_ratio").unwrap_or(&"0.5".to_string()).clone();
        context.insert("max_distance_m".into(), max_distance.to_string());
        context.insert("min_coverage_required".into(), format!("{:.1}", min_coverage.parse::<f64>().unwrap_or(0.5) * 100.0));
        
        context
    }
}

impl Analyzer for PedestrianCompletenessAnalyzer {
    fn name(&self) -> &str {
        "pedestrian_completeness"
    }

    fn version(&self) -> &str {
        "0.2.0"
    }

    fn description(&self) -> &str {
        "Identifies roads that are missing `sidewalk` tags or separately mapped sidewalks."
    }

    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();
        
        // Check if debug mode is enabled
        let debug_mode = std::env::var("OOPS_DEBUG").is_ok();

        // Phase 1: Build a spatial index of all sidewalks
        let sidewalks: Vec<_> = data
            .ways
            .values()
            .filter(|way| {
                way.tags.get("highway") == Some(&"footway".into())
                    && way.tags.get("footway") == Some(&"sidewalk".into())
            })
            .filter_map(|way| {
                way_to_linestring(way, data).map(|linestring| WayWithLineString {
                        way: way.clone(),
                        linestring,
                    })
            })
            .collect();
        let sidewalk_rtree = RTree::bulk_load(sidewalks);

        if debug_mode {
            eprintln!("=== Pedestrian Completeness Debug ===");
            eprintln!("Built spatial index with {} separate sidewalks", sidewalk_rtree.size());
            eprintln!("Highway-specific distance thresholds:");
            eprintln!("  residential/unclassified: {}m", self.get_distance_threshold("residential"));
            eprintln!("  tertiary: {}m", self.get_distance_threshold("tertiary"));
            eprintln!("  secondary: {}m", self.get_distance_threshold("secondary"));
            eprintln!("  primary: {}m", self.get_distance_threshold("primary"));
            eprintln!("Max angle diff: {}°", 
                self.config.get("max_angle_diff").unwrap_or(&"30.0".to_string())
            );
            eprintln!();
        }

        // Phase 2: Analyze each road
        for way in data.ways.values() {
            if !way.tags.contains_key("highway") {
                if debug_mode {
                    eprintln!("SKIP {}: not a highway", way.id);
                }
                continue;
            }

            let way_name = way.tags.get("name").unwrap_or(&"unnamed".to_string()).clone();
            let highway_type = way.tags.get("highway").unwrap_or(&"unknown".to_string()).clone();
            
            if !self.should_analyze_way(way) {
                if debug_mode {
                    eprintln!("SKIP {}: {} - not a relevant road type", 
                        way.id, highway_type);
                }
                continue;
            }

            if self.has_sidewalk_tag(way) {
                let sidewalk_info = if let Some(sidewalk) = way.tags.get("sidewalk") {
                    format!("sidewalk={}", sidewalk)
                } else {
                    let mut parts = Vec::new();
                    if let Some(left) = way.tags.get("sidewalk:left") {
                        parts.push(format!("left={}", left));
                    }
                    if let Some(right) = way.tags.get("sidewalk:right") {
                        parts.push(format!("right={}", right));
                    }
                    parts.join(", ")
                };
                if debug_mode {
                    eprintln!("SKIP {}: {} '{}' - has sidewalk tag: {}", 
                        way.id, highway_type, way_name, sidewalk_info);
                }
                continue;
            }

            let way_in_query_area = way.nodes.iter().any(|&node_id| {
                if let Some(node) = data.get_node(node_id) {
                    query.area.contains_point(node.lon, node.lat)
                } else {
                    false
                }
            });

            if !way_in_query_area {
                if debug_mode {
                    eprintln!("SKIP {}: {} '{}' - outside query area", 
                        way.id, highway_type, way_name);
                }
                continue;
            }

            // Phase 3: Proximity analysis for roads without sidewalk tags
            let max_distance = self.get_distance_threshold(&highway_type);
            let max_angle_diff = self.config.get("max_angle_diff")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(30.0);
            let min_coverage_ratio = self.config.get("min_coverage_ratio")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.5);
                
            let nearby_sidewalks = find_parallel_adjacent_ways(
                way, 
                &sidewalk_rtree, 
                max_distance, 
                max_angle_diff, 
                data
            );

            // Calculate coverage ratio if sidewalks exist
            let coverage_ratio = if nearby_sidewalks.is_empty() {
                0.0
            } else {
                calculate_sidewalk_coverage(way, &nearby_sidewalks, max_distance, data)
            };

            if coverage_ratio < min_coverage_ratio {
                if debug_mode {
                    if nearby_sidewalks.is_empty() {
                        eprintln!("FLAG {}: {} '{}' - no nearby parallel sidewalks found (max_distance: {}m)", 
                            way.id, highway_type, way_name, max_distance);
                    } else {
                        let sidewalk_ids: Vec<String> = nearby_sidewalks.iter()
                            .map(|sw| sw.way.id.to_string())
                            .collect();
                        eprintln!("FLAG {}: {} '{}' - insufficient sidewalk coverage: {:.1}% < {:.1}% required (max_distance: {}m, sidewalks: [{}])", 
                            way.id, highway_type, way_name,
                            coverage_ratio * 100.0, 
                            min_coverage_ratio * 100.0,
                            max_distance,
                            sidewalk_ids.join(", ")
                        );
                    }
                }
                
                let mut result = AnalysisResult::new(way.id, ElementType::Way);
                
                let (message, recommendation) = if nearby_sidewalks.is_empty() {
                    (
                        "Road has no sidewalk tags and no nearby parallel sidewalks were found.".to_string(),
                        "Add a sidewalk=* tag (none/left/right/both/separate) or map adjacent sidewalk ways as highway=footway + footway=sidewalk.".to_string()
                    )
                } else {
                    (
                        format!("Road has insufficient sidewalk coverage ({:.1}% covered, {:.1}% required).", 
                            coverage_ratio * 100.0, 
                            min_coverage_ratio * 100.0),
                        "Extend existing sidewalk mapping to cover more of this road, or add sidewalk=* tags to indicate actual sidewalk presence.".to_string()
                    )
                };
                
                let finding = Finding {
                    category: ProblemCategory::Missing,
                    severity: Severity::Low,
                    message: Some(message),
                    recommendation: Some(recommendation),
                    context: self.create_finding_context(way, &nearby_sidewalks, coverage_ratio),
                };
                result.add_finding(finding);
                results.push(result);
            } else if debug_mode {
                let sidewalk_ids: Vec<String> = nearby_sidewalks.iter()
                    .map(|sw| sw.way.id.to_string())
                    .collect();
                eprintln!("OK   {}: {} '{}' - adequate sidewalk coverage: {:.1}% >= {:.1}% required (max_distance: {}m, sidewalks: [{}])", 
                    way.id, highway_type, way_name,
                    coverage_ratio * 100.0,
                    min_coverage_ratio * 100.0,
                    max_distance,
                    sidewalk_ids.join(", ")
                );
            }
        }

        results.sort_by_key(|r| r.element_id);
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_data() -> OsmData {
        let mut data = OsmData::new();

        // Nodes
        data.add_node(OsmNode::new(1, -111.9, 40.75));
        data.add_node(OsmNode::new(2, -111.85, 40.75));
        data.add_node(OsmNode::new(3, -111.9, 40.76));
        data.add_node(OsmNode::new(4, -111.85, 40.76));
        data.add_node(OsmNode::new(5, -111.9, 40.77));
        data.add_node(OsmNode::new(6, -111.85, 40.77));

        // Way 1: Residential road WITHOUT sidewalk tag (should be flagged)
        let mut residential_way = OsmWay::new(1, vec![1, 2]);
        residential_way.tags.insert("highway".into(), "residential".into());
        residential_way.tags.insert("name".into(), "Elm Street".into());
        data.ways.insert(1, residential_way);

        // Way 2: Primary road WITH sidewalk=both tag (should NOT be flagged)
        let mut primary_way = OsmWay::new(2, vec![3, 4]);
        primary_way.tags.insert("highway".into(), "primary".into());
        primary_way.tags.insert("name".into(), "Main Street".into());
        primary_way.tags.insert("sidewalk".into(), "both".into());
        data.ways.insert(2, primary_way);

        // Way 3: Motorway WITHOUT sidewalk tag (should NOT be flagged)
        let mut motorway_way = OsmWay::new(3, vec![5, 6]);
        motorway_way.tags.insert("highway".into(), "motorway".into());
        data.ways.insert(3, motorway_way);

        data
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = PedestrianCompletenessAnalyzer::new();
        assert_eq!(analyzer.name(), "pedestrian_completeness");
        assert_eq!(analyzer.version(), "0.2.0");
    }

    #[test]
    fn test_should_analyze_way() {
        let analyzer = PedestrianCompletenessAnalyzer::new();
        let mut way = OsmWay::new(1, vec![1, 2]);

        // Should analyze
        way.tags.insert("highway".into(), "residential".into());
        assert!(analyzer.should_analyze_way(&way));

        // Should not analyze
        way.tags.insert("highway".into(), "motorway".into());
        assert!(!analyzer.should_analyze_way(&way));

        way.tags.insert("highway".into(), "service".into());
        assert!(!analyzer.should_analyze_way(&way));
    }

    #[test]
    fn test_analysis_workflow() {
        let data = create_test_data();
        let analyzer = PedestrianCompletenessAnalyzer::new();
        let bbox = BoundingBox::new(-112.0, 40.0, -111.8, 40.8);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        assert_eq!(results.len(), 1);
        let result = &results[0];
        assert_eq!(result.element_id, 1); // Residential way ID
        assert_eq!(result.findings.len(), 1);
        let finding = &result.findings[0];
        assert_eq!(finding.category, ProblemCategory::Missing);
        assert!(finding.message.as_ref().unwrap().contains("no sidewalk tags") || 
               finding.message.as_ref().unwrap().contains("no nearby parallel sidewalks"));
    }

    #[test]
    fn test_analysis_with_separate_sidewalk() {
        let mut data = OsmData::new();

        // Create nodes for road and parallel sidewalk
        data.add_node(OsmNode::new(1, -111.9, 40.75));
        data.add_node(OsmNode::new(2, -111.85, 40.75));
        data.add_node(OsmNode::new(3, -111.9, 40.750001)); // ~11m north
        data.add_node(OsmNode::new(4, -111.85, 40.750001));

        // Road without sidewalk tag
        let mut road = OsmWay::new(1, vec![1, 2]);
        road.tags.insert("highway".into(), "residential".into());
        road.tags.insert("name".into(), "Test Street".into());
        data.ways.insert(1, road);

        // Parallel sidewalk
        let mut sidewalk = OsmWay::new(2, vec![3, 4]);
        sidewalk.tags.insert("highway".into(), "footway".into());
        sidewalk.tags.insert("footway".into(), "sidewalk".into());
        data.ways.insert(2, sidewalk);

        let analyzer = PedestrianCompletenessAnalyzer::new();
        let bbox = BoundingBox::new(-112.0, 40.0, -111.8, 40.8);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        // Should find no issues since there's a parallel sidewalk nearby
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_analysis_with_perpendicular_footway() {
        let mut data = OsmData::new();

        // Create nodes
        data.add_node(OsmNode::new(1, -111.9, 40.75));
        data.add_node(OsmNode::new(2, -111.85, 40.75));
        data.add_node(OsmNode::new(3, -111.875, 40.749)); // Perpendicular crossing
        data.add_node(OsmNode::new(4, -111.875, 40.751));

        // Road without sidewalk tag
        let mut road = OsmWay::new(1, vec![1, 2]);
        road.tags.insert("highway".into(), "residential".into());
        data.ways.insert(1, road);

        // Perpendicular footway (crossing, not sidewalk)
        let mut footway = OsmWay::new(2, vec![3, 4]);
        footway.tags.insert("highway".into(), "footway".into());
        footway.tags.insert("footway".into(), "sidewalk".into());
        data.ways.insert(2, footway);

        let analyzer = PedestrianCompletenessAnalyzer::new();
        let bbox = BoundingBox::new(-112.0, 40.0, -111.8, 40.8);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        // Should flag the road since the footway isn't parallel
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].element_id, 1);
    }

    #[test]
    fn test_configurable_parameters() {
        let mut config = AnalyzerConfig::new("pedestrian_completeness");
        config.set("distance_primary", "40.0");
        config.set("distance_secondary", "35.0");
        config.set("max_angle_diff", "45.0");
        
        let analyzer = PedestrianCompletenessAnalyzer::with_config(config);
        
        assert_eq!(analyzer.get_distance_threshold("primary"), 40.0);
        assert_eq!(analyzer.get_distance_threshold("secondary"), 35.0);
        assert_eq!(analyzer.get_distance_threshold("residential"), 20.0); // Should use fallback
        assert_eq!(analyzer.config.get("max_angle_diff").unwrap(), "45.0");
    }

    #[test]
    fn test_highway_specific_distance_thresholds() {
        let analyzer = PedestrianCompletenessAnalyzer::new();
        
        assert_eq!(analyzer.get_distance_threshold("residential"), 20.0);
        assert_eq!(analyzer.get_distance_threshold("unclassified"), 20.0);
        assert_eq!(analyzer.get_distance_threshold("tertiary"), 25.0);
        assert_eq!(analyzer.get_distance_threshold("secondary"), 30.0);
        assert_eq!(analyzer.get_distance_threshold("primary"), 35.0);
        assert_eq!(analyzer.get_distance_threshold("unknown"), 20.0); // fallback
    }

    #[test]
    fn test_sidewalk_tag_detection() {
        let analyzer = PedestrianCompletenessAnalyzer::new();
        
        // Test generic sidewalk tag
        let mut way1 = OsmWay::new(1, vec![1, 2]);
        way1.tags.insert("highway".into(), "residential".into());
        way1.tags.insert("sidewalk".into(), "both".into());
        assert!(analyzer.has_sidewalk_tag(&way1));
        
        // Test specific sidewalk:left tag
        let mut way2 = OsmWay::new(2, vec![1, 2]);
        way2.tags.insert("highway".into(), "secondary".into());
        way2.tags.insert("sidewalk:left".into(), "no".into());
        assert!(analyzer.has_sidewalk_tag(&way2));
        
        // Test specific sidewalk:right tag
        let mut way3 = OsmWay::new(3, vec![1, 2]);
        way3.tags.insert("highway".into(), "secondary".into());
        way3.tags.insert("sidewalk:right".into(), "separate".into());
        assert!(analyzer.has_sidewalk_tag(&way3));
        
        // Test both sidewalk:left and sidewalk:right tags
        let mut way4 = OsmWay::new(4, vec![1, 2]);
        way4.tags.insert("highway".into(), "primary".into());
        way4.tags.insert("sidewalk:left".into(), "yes".into());
        way4.tags.insert("sidewalk:right".into(), "separate".into());
        assert!(analyzer.has_sidewalk_tag(&way4));
        
        // Test way without sidewalk tags
        let mut way5 = OsmWay::new(5, vec![1, 2]);
        way5.tags.insert("highway".into(), "residential".into());
        assert!(!analyzer.has_sidewalk_tag(&way5));
    }
}