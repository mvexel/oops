//! Missing tag detection analyzer.
//!
//! This analyzer identifies roads that are missing required tags,
//! with severity levels based on highway classification.

use oops_core::analysis::*;
use oops_core::data::OsmData;
use oops_core::severity::determine_severity_by_highway;
use std::collections::HashMap;

/// Highway-aware severity detector - ANALYZER-SPECIFIC logic
pub struct MissingTagDetector {
    tag: String,
    message_template: String,
}

impl MissingTagDetector {
    pub fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            message_template: "Road is missing the '{}' tag".to_string(),
        }
    }

    pub fn with_message_template(mut self, template: &str) -> Self {
        self.message_template = template.to_string();
        self
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

impl ProblemDetector for MissingTagDetector {
    fn detect_problems(
        &self,
        context: &EnrichedContext,
        _data: &OsmData,
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
        let severity = determine_severity_by_highway(highway_type);

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
            context: finding_context,
        }]
    }
}