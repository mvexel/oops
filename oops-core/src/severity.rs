//! Severity determination utilities for OSM quality issues.
//!
//! This module provides reusable logic for determining issue severity
//! based on OSM element characteristics like highway type.

use crate::analysis::Severity;

/// Determine severity based on highway type.
/// 
/// Higher-tier roads (motorway, trunk) get higher severity for missing tags
/// since they impact more users and routing systems.
///
/// # Arguments
/// * `highway_type` - The highway tag value (e.g., "primary", "residential")
///
/// # Returns
/// Appropriate severity level for the highway type
pub fn determine_severity_by_highway(highway_type: &str) -> Severity {
    match highway_type {
        "motorway" | "trunk" => Severity::High,
        "primary" | "secondary" => Severity::Medium,
        _ => Severity::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_severity_highways() {
        assert_eq!(determine_severity_by_highway("motorway"), Severity::High);
        assert_eq!(determine_severity_by_highway("trunk"), Severity::High);
    }

    #[test]
    fn test_medium_severity_highways() {
        assert_eq!(determine_severity_by_highway("primary"), Severity::Medium);
        assert_eq!(determine_severity_by_highway("secondary"), Severity::Medium);
    }

    #[test]
    fn test_low_severity_highways() {
        assert_eq!(determine_severity_by_highway("tertiary"), Severity::Low);
        assert_eq!(determine_severity_by_highway("residential"), Severity::Low);
        assert_eq!(determine_severity_by_highway("unclassified"), Severity::Low);
        assert_eq!(determine_severity_by_highway("unknown"), Severity::Low);
    }
}