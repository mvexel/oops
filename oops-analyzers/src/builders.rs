//! Analyzer builders that compose generic + specific components.
//!
//! These functions demonstrate clean separation between generic core components
//! and analyzer-specific detection logic.

use crate::intersection::IntersectionDetector;
use crate::missing_tag::MissingTagDetector;
use crate::sidewalk_coverage::SidewalkCoverageDetector;
use oops_core::analysis::*;

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
        Box::new(MissingTagDetector::new("maxspeed").with_message_template(
            "Road is missing maxspeed tag - this can cause routing inconsistencies",
        )),
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
            SpatialContext::with_highway_thresholds(|tags| {
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

/// Build intersection analyzer using composable architecture
pub fn build_intersection_analyzer(cluster_distance: f64) -> Analyzer {
    Analyzer::new(
        "intersection_detection",
        "1.0.0", 
        "Detects and classifies road intersections including complex and clustered intersections",
        // Selector: Use new IntersectionSelector to find intersection clusters
        Box::new(IntersectionSelector::new(cluster_distance)),
        // Context: Use new IntersectionContextProvider for clustering and metadata
        Box::new(IntersectionContextProvider::new()),
        // Detector: Use new IntersectionDetector to analyze intersection properties
        Box::new(IntersectionDetector::new(3, true)), // Report 3+ way intersections, include complex
    )
}

/// Get all analyzers - demonstrates clean separation
pub fn get_all_analyzers(cluster_distance: f64) -> Vec<Analyzer> {
    vec![
        build_missing_maxspeed_analyzer(), // Generic core + specific detector
        build_pedestrian_completeness_analyzer(), // Generic core + specific spatial detector
        build_intersection_analyzer(cluster_distance), // Composable intersection detection with clustering
    ]
}