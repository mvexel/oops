//! Analyzers built with the new composable architecture
//!
//! This demonstrates the proper separation of concerns:
//! - Core provides GENERIC components (WaySelector, MissingTagDetector)
//! - Analyzers provide SPECIFIC logic (SidewalkCoverageDetector, HighwaySeverityDetector)

use geo::Point;
use oops_core::analysis::*;
use oops_core::graph::build_node_to_ways_index;
use oops_core::intersections::find_logical_intersections;
use oops_core::spatial::calculate_sidewalk_coverage;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct IntersectionNodePoint {
    pub node_id: i64,
    pub point: Point<f64>,
    pub degree: usize,
}

#[derive(Clone, Debug)]
pub struct LogicalIntersection {
    pub representative_node: i64,
    pub node_cluster: Vec<i64>,
}

// ============================================================================
// ANALYZER-SPECIFIC DETECTORS (belong here, not in core!)
// ============================================================================

/// Detects inadequate sidewalk coverage - ANALYZER-SPECIFIC logic
pub struct SidewalkCoverageDetector {
    min_coverage_ratio: f64,
    enable_maproulette_metadata: bool,
}

impl SidewalkCoverageDetector {
    pub fn new(min_coverage_ratio: f64) -> Self {
        Self {
            min_coverage_ratio,
            enable_maproulette_metadata: true, // Enable by default for backward compatibility
        }
    }

    pub fn with_maproulette_metadata(mut self, enable: bool) -> Self {
        self.enable_maproulette_metadata = enable;
        self
    }

    /// Add MapRoulette-specific metadata for tailored instructions (from legacy analyzer)
    fn add_maproulette_metadata(
        &self,
        context: &mut HashMap<String, String>,
        enriched_context: &EnrichedContext,
        coverage_ratio: f64,
    ) {
        // Extract available sidewalk tags
        let mut available_tags = Vec::new();
        if let Some(sidewalk) = enriched_context.element.tags.get("sidewalk") {
            available_tags.push(format!("sidewalk={}", sidewalk));
        }
        if let Some(left) = enriched_context.element.tags.get("sidewalk:left") {
            available_tags.push(format!("sidewalk:left={}", left));
        }
        if let Some(right) = enriched_context.element.tags.get("sidewalk:right") {
            available_tags.push(format!("sidewalk:right={}", right));
        }

        context.insert(
            "available_sidewalk_tags".to_string(),
            available_tags.join(", "),
        );

        // Add coverage requirements
        let highway_type = enriched_context
            .element
            .tags
            .get("highway")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        context.insert(
            "min_coverage_required".to_string(),
            format!("{:.1}", self.min_coverage_ratio * 100.0),
        );

        // Add distance information (would need access to ParallelWayContext configuration)
        // For now, use legacy defaults based on highway type
        let distance_used = match highway_type {
            "residential" | "unclassified" => 20.0,
            "tertiary" => 25.0,
            "secondary" => 30.0,
            "primary" => 35.0,
            _ => 20.0,
        };
        context.insert("max_distance_m".to_string(), distance_used.to_string());

        // Calculate directional coverage (simplified - would need proper geometry analysis)
        let sidewalk_count = enriched_context.spatial_context.nearby_ways.len();
        let forward_coverage = if sidewalk_count > 0 {
            coverage_ratio * 0.6
        } else {
            0.0
        };
        let backward_coverage = if sidewalk_count > 0 {
            coverage_ratio * 0.4
        } else {
            0.0
        };

        context.insert(
            "forward_coverage_percent".to_string(),
            format!("{:.1}", forward_coverage * 100.0),
        );
        context.insert(
            "backward_coverage_percent".to_string(),
            format!("{:.1}", backward_coverage * 100.0),
        );

        // Add nearby sidewalk IDs if any
        if !enriched_context.spatial_context.nearby_ways.is_empty() {
            let sidewalk_ids: Vec<String> = enriched_context
                .spatial_context
                .nearby_ways
                .iter()
                .map(|way| way.way_id.to_string())
                .collect();
            context.insert("nearby_sidewalk_ids".to_string(), sidewalk_ids.join(","));
        }
    }
}

impl ProblemDetector for SidewalkCoverageDetector {
    fn detect_problems(
        &self,
        context: &EnrichedContext,
        data: &oops_core::data::OsmData,
    ) -> Vec<Finding> {
        // Only analyze roads that don't have sidewalk tags
        let has_sidewalk_tags = context.element.tags.contains_key("sidewalk")
            || context.element.tags.contains_key("sidewalk:left")
            || context.element.tags.contains_key("sidewalk:right");

        if has_sidewalk_tags {
            return Vec::new();
        }

        // The target way must exist in the data
        let Some(target_way) = data.ways.get(&context.element.element_id) else {
            return Vec::new();
        };

        // Get the parallel sidewalks from the context with proper filtering
        let all_nearby_sidewalks: Vec<_> = context
            .spatial_context
            .nearby_ways
            .iter()
            .filter_map(|nearby| data.ways.get(&nearby.way_id))
            .filter_map(|way| {
                oops_core::spatial::way_to_linestring(way, data).map(|linestring| {
                    oops_core::spatial::WayWithLineString {
                        way: way.clone(),
                        linestring,
                    }
                })
            })
            .collect();

        // Apply proper parallel filtering that excludes perpendicular crossings
        let Some(target_linestring) = oops_core::spatial::way_to_linestring(target_way, data)
        else {
            return Vec::new();
        };

        let parallel_sidewalks: Vec<_> = all_nearby_sidewalks
            .into_iter()
            .filter(|candidate| {
                // Exclude ways that cross geometrically (perpendicular crossings)
                !oops_core::spatial::ways_cross_geometrically(target_way, &candidate.way, data) &&
                // Then check parallelism with enhanced projection-based algorithm
                oops_core::spatial::are_ways_parallel(&target_linestring, &candidate.linestring, 30.0)
            })
            .collect();

        // Create references for the function call
        let parallel_sidewalk_refs: Vec<_> = parallel_sidewalks.iter().collect();

        // Get the highway-specific distance threshold
        let highway_type = target_way
            .tags
            .get("highway")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let distance_threshold = match highway_type {
            "residential" | "unclassified" => 20.0,
            "tertiary" => 25.0,
            "secondary" => 30.0,
            "primary" => 35.0,
            _ => 20.0,
        };

        // Use the new, more accurate coverage calculation
        let coverage_ratio = calculate_sidewalk_coverage(
            target_way,
            &parallel_sidewalk_refs,
            distance_threshold,
            data,
        );

        if coverage_ratio >= self.min_coverage_ratio {
            return Vec::new();
        }

        let mut finding_context = HashMap::new();
        finding_context.insert(
            "element_id".to_string(),
            context.element.element_id.to_string(),
        );
        finding_context.insert(
            "coverage_ratio".to_string(),
            format!("{:.3}", coverage_ratio),
        );
        finding_context.insert(
            "coverage_percent".to_string(),
            format!("{:.1}", coverage_ratio * 100.0),
        );
        finding_context.insert(
            "sidewalk_count".to_string(),
            parallel_sidewalks.len().to_string(),
        );
        finding_context.insert(
            "nearby_sidewalk_count".to_string(),
            parallel_sidewalks.len().to_string(),
        );

        // Add highway information
        if let Some(highway_type) = context.element.tags.get("highway") {
            finding_context.insert("highway_type".to_string(), highway_type.clone());
        }

        // Add way name if available
        if let Some(name) = context.element.tags.get("name") {
            finding_context.insert("way_name".to_string(), name.clone());
        }

        // Add MapRoulette-specific metadata if enabled
        if self.enable_maproulette_metadata {
            self.add_maproulette_metadata(&mut finding_context, context, coverage_ratio);
        }

        let message = if parallel_sidewalks.is_empty() {
            "Road has no sidewalk tags and no nearby parallel sidewalks were found".to_string()
        } else {
            format!(
                "Road has insufficient sidewalk coverage ({:.1}% covered, {:.1}% required)",
                coverage_ratio * 100.0,
                self.min_coverage_ratio * 100.0
            )
        };

        let recommendation = if parallel_sidewalks.is_empty() {
            "Add a sidewalk=* tag or map adjacent sidewalk ways".to_string()
        } else {
            "Extend existing sidewalk mapping or add sidewalk=* tags".to_string()
        };

        vec![Finding {
            category: ProblemCategory::Missing,
            severity: Severity::Low,
            message: Some(message),
            recommendation: Some(recommendation),
            context: finding_context,
        }]
    }
}

/// Highway-aware severity detector - ANALYZER-SPECIFIC logic
pub struct HighwaySeverityDetector {
    tag: String,
    message_template: String,
    recommendation_template: String,
}

impl HighwaySeverityDetector {
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            message_template: "Road is missing the '{}' tag".to_string(),
            recommendation_template: "Add the '{}' tag with an appropriate value".to_string(),
        }
    }

    pub fn with_message_template(mut self, template: &str) -> Self {
        self.message_template = template.to_string();
        self
    }

    fn determine_severity(&self, highway_type: &str) -> Severity {
        match highway_type {
            "motorway" | "trunk" => Severity::High,
            "primary" | "secondary" => Severity::Medium,
            _ => Severity::Low,
        }
    }

    fn create_detailed_message(&self, highway_type: &str, road_name: Option<&str>) -> String {
        let road_identifier = road_name
            .map(|name| format!("\"{}\"", name))
            .unwrap_or_else(|| "road".to_string());

        format!(
            "{} {} is missing {} tag - this can cause routing inconsistencies",
            highway_type.replace("_", " ").to_uppercase(),
            road_identifier,
            self.tag
        )
    }
}

impl ProblemDetector for HighwaySeverityDetector {
    fn detect_problems(
        &self,
        context: &EnrichedContext,
        _data: &oops_core::data::OsmData,
    ) -> Vec<Finding> {
        // Check if the required tag is missing
        if context.element.tags.contains_key(&self.tag) {
            return Vec::new();
        }

        let highway_type = context
            .element
            .tags
            .get("highway")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let road_name = context.element.tags.get("name");
        let severity = self.determine_severity(highway_type);

        let mut finding_context = HashMap::new();
        finding_context.insert(
            "element_id".to_string(),
            context.element.element_id.to_string(),
        );
        finding_context.insert("highway_type".to_string(), highway_type.to_string());

        if let Some(name) = road_name {
            finding_context.insert("road_name".to_string(), name.clone());
        }

        vec![Finding {
            category: ProblemCategory::Missing,
            severity,
            message: Some(
                self.create_detailed_message(highway_type, road_name.map(|s| s.as_str())),
            ),
            recommendation: Some(self.recommendation_template.replace("{}", &self.tag)),
            context: finding_context,
        }]
    }
}

// ============================================================================
// ANALYZER BUILDERS (compose generic + specific components)
// ============================================================================

/// Build missing maxspeed analyzer - uses GENERIC core + SPECIFIC detector
pub fn build_missing_maxspeed_analyzer() -> Analyzer {
    Analyzer::new(
        "missing_maxspeed",
        "1.0.0",
        "Identifies main roads missing maxspeed tags",
        // GENERIC component from core
        Box::new(
            WaySelector::new()
                .highway_types(vec![
                    "primary",
                    "secondary",
                    "tertiary",
                    "trunk",
                    "motorway",
                ])
                .exclude_with_tag("maxspeed"),
        ),
        // GENERIC component from core
        Box::new(NoContext),
        // ANALYZER-SPECIFIC component (lives here, not in core!)
        Box::new(
            HighwaySeverityDetector::new("maxspeed").with_message_template(
                "Road is missing maxspeed tag - this can cause routing inconsistencies",
            ),
        ),
    )
}

/// Build pedestrian completeness analyzer - complex spatial analysis
pub fn build_pedestrian_completeness_analyzer() -> Analyzer {
    Analyzer::new(
        "pedestrian_completeness",
        "1.0.0",
        "Identifies roads missing sidewalk infrastructure",
        // GENERIC component from core
        Box::new(
            WaySelector::new()
                .highway_types(vec![
                    "primary",
                    "secondary",
                    "tertiary",
                    "residential",
                    "unclassified",
                ])
                .exclude_with_tag("sidewalk"),
        ),
        // GENERIC component from core with sidewalk-specific filter
        Box::new(
            oops_core::analysis::SpatialContext::with_highway_thresholds(|tags| {
                // Expanded pedestrian infrastructure detection logic
                let highway = tags.get("highway").map(|s| s.as_str());
                let footway = tags.get("footway").map(|s| s.as_str());
                let foot = tags.get("foot").map(|s| s.as_str());
                let bicycle = tags.get("bicycle").map(|s| s.as_str());

                match highway {
                    // Traditional sidewalks and crossings
                    Some("footway") => {
                        footway == Some("sidewalk") ||  // explicit sidewalks
                    footway == Some("crossing") ||  // pedestrian crossings
                    footway.is_none() // generic footways (likely sidewalks)
                    }

                    // Paths - both sidewalks and mixed-use
                    Some("path") => {
                        footway == Some("sidewalk") ||  // explicit sidewalks
                    (bicycle == Some("designated") && foot != Some("no")) // mixed-use unless foot=no
                    }

                    // Cycleways that allow pedestrians
                    Some("cycleway") => {
                        foot == Some("yes")
                            || foot == Some("designated")
                            || foot == Some("permissive")
                            || foot.is_none() // Default assumption: cycleways usually allow pedestrians
                    }

                    _ => false,
                }
            }),
        ),
        // ANALYZER-SPECIFIC component (lives here, not in core!)
        Box::new(SidewalkCoverageDetector::new(0.5)), // Use legacy default (50% coverage)
    )
}

// ============================================================================
// INTERSECTION-SPECIFIC COMPONENTS
// ============================================================================

// ============================================================================
// INTERSECTION ANALYZER (V2)
// ============================================================================

#[derive(Debug, Clone)]
pub struct Intersection {
    pub representative_node: i64,
    pub node_cluster: Vec<i64>,
    pub degree: usize,
    pub intersection_type: String,
    pub coordinates: Point<f64>,
}

pub struct IntersectionAnalyzer {
    cluster_distance: f64,
}

impl IntersectionAnalyzer {
    pub fn new(cluster_distance: f64) -> Self {
        Self { cluster_distance }
    }

    pub fn analyze(&self, osm_data: &oops_core::data::OsmData) -> Vec<Intersection> {
        // Step 1: Use the new, efficient R-Tree-based function to find logical intersections.
        let logical_intersections = find_logical_intersections(osm_data, self.cluster_distance);
        let node_to_ways = build_node_to_ways_index(osm_data);

        // Step 2: Process the results into the final `Intersection` struct.
        logical_intersections
            .into_iter()
            .map(|li| {
                let mut connected_ways = HashSet::new();
                for &node_id in &li.node_cluster {
                    if let Some(way_ids) = node_to_ways.get(&node_id) {
                        for &way_id in way_ids {
                            connected_ways.insert(way_id);
                        }
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

/// Get all analyzers - demonstrates clean separation
pub fn get_all_analyzers() -> Vec<Analyzer> {
    vec![
        build_missing_maxspeed_analyzer(), // Generic core + specific detector
        build_pedestrian_completeness_analyzer(), // Generic core + specific spatial detector
    ]
}
