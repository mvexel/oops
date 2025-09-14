//! Sidewalk coverage detection analyzer.
//!
//! This analyzer identifies roads with inadequate sidewalk coverage by examining
//! both OSM tags and spatial relationships to nearby sidewalk ways.

use oops_core::analysis::*;
use oops_core::data::OsmData;
use oops_core::spatial::{calculate_sidewalk_coverage, way_to_linestring, ways_cross_geometrically, 
                         are_ways_geometrically_parallel, WayWithLineString};
use oops_core::tags::evaluate_sidewalk_tags;
use std::collections::HashMap;

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
        data: &OsmData,
    ) -> Vec<Finding> {
        // Check for sidewalk tags and evaluate them
        let has_sidewalk_tags = context.element.tags.contains_key("sidewalk")
            || context.element.tags.contains_key("sidewalk:left")
            || context.element.tags.contains_key("sidewalk:right");

        if has_sidewalk_tags {
            // Evaluate sidewalk tags to determine coverage
            let tag_coverage = evaluate_sidewalk_tags(&context.element.tags);
            if tag_coverage >= self.min_coverage_ratio {
                return Vec::new(); // Sufficient coverage from tags
            }
            // Continue with spatial analysis to supplement tag-based coverage
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
                way_to_linestring(way, data).map(|linestring| {
                    WayWithLineString {
                        way: way.clone(),
                        linestring,
                    }
                })
            })
            .collect();

        // Apply proper parallel filtering that excludes perpendicular crossings
        let Some(target_linestring) = way_to_linestring(target_way, data)
        else {
            return Vec::new();
        };

        let parallel_sidewalks: Vec<_> = all_nearby_sidewalks
            .into_iter()
            .filter(|candidate| {
                // Exclude ways that cross geometrically (perpendicular crossings)
                !ways_cross_geometrically(target_way, &candidate.way, data) &&
                // Then check parallelism with enhanced projection-based algorithm
                are_ways_geometrically_parallel(&target_linestring, &candidate.linestring, 30.0)
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

        vec![Finding {
            category: ProblemCategory::Missing,
            severity: Severity::Low,
            message: Some(message),
            context: finding_context,
        }]
    }
}