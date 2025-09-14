//! Generic OSM tag evaluation utilities.
//!
//! This module contains reusable functions for interpreting and evaluating
//! OSM tag values across different contexts.

use std::collections::HashMap;

/// Evaluate sidewalk tags to determine coverage percentage.
/// 
/// This function interprets standard OSM sidewalk tags and returns a coverage
/// ratio between 0.0 and 1.0, where:
/// - 0.0 = no sidewalk coverage
/// - 0.5 = single-side coverage (left or right)  
/// - 1.0 = full coverage (both sides)
///
/// # Arguments
/// * `tags` - HashMap of OSM tags to evaluate
///
/// # Returns
/// Coverage ratio as f64 between 0.0 and 1.0
pub fn evaluate_sidewalk_tags(tags: &HashMap<String, String>) -> f64 {
    // Check main sidewalk tag
    if let Some(sidewalk_value) = tags.get("sidewalk") {
        match sidewalk_value.as_str() {
            "both" => return 1.0,
            "left" | "right" => return 0.5,
            "no" | "none" => return 0.0,
            "separate" => {
                // For separate sidewalks, we should continue with spatial analysis
                return 0.0; // Let spatial analysis determine coverage
            }
            _ => {} // Unknown value, continue checking
        }
    }

    // Check individual sidewalk:left/right tags
    let left_coverage = match tags.get("sidewalk:left").map(|s| s.as_str()) {
        Some("yes") => 0.5,
        Some("no") | Some("none") => 0.0,
        Some("separate") => 0.0, // Let spatial analysis handle
        _ => 0.0,                // Unknown or missing
    };

    let right_coverage = match tags.get("sidewalk:right").map(|s| s.as_str()) {
        Some("yes") => 0.5,
        Some("no") | Some("none") => 0.0,
        Some("separate") => 0.0, // Let spatial analysis handle
        _ => 0.0,                // Unknown or missing
    };

    left_coverage + right_coverage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_sidewalk_both() {
        let mut tags = HashMap::new();
        tags.insert("sidewalk".to_string(), "both".to_string());
        
        assert_eq!(evaluate_sidewalk_tags(&tags), 1.0);
    }

    #[test]
    fn test_evaluate_sidewalk_single_side() {
        let mut tags = HashMap::new();
        tags.insert("sidewalk".to_string(), "left".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.5);

        tags.insert("sidewalk".to_string(), "right".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.5);
    }

    #[test]
    fn test_evaluate_sidewalk_none() {
        let mut tags = HashMap::new();
        tags.insert("sidewalk".to_string(), "no".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.0);

        tags.insert("sidewalk".to_string(), "none".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.0);
    }

    #[test]
    fn test_evaluate_sidewalk_separate() {
        let mut tags = HashMap::new();
        tags.insert("sidewalk".to_string(), "separate".to_string());
        // Should return 0.0 to let spatial analysis handle it
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.0);
    }

    #[test]
    fn test_evaluate_sidewalk_left_right_tags() {
        let mut tags = HashMap::new();
        tags.insert("sidewalk:left".to_string(), "yes".to_string());
        tags.insert("sidewalk:right".to_string(), "yes".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 1.0);

        tags.clear();
        tags.insert("sidewalk:left".to_string(), "yes".to_string());
        tags.insert("sidewalk:right".to_string(), "no".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.5);

        tags.clear();
        tags.insert("sidewalk:left".to_string(), "no".to_string());
        tags.insert("sidewalk:right".to_string(), "separate".to_string());
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.0);
    }

    #[test]
    fn test_evaluate_sidewalk_no_tags() {
        let tags = HashMap::new();
        assert_eq!(evaluate_sidewalk_tags(&tags), 0.0);
    }
}