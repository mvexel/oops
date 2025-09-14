//! Intersection detection and analysis.
//!
//! This analyzer identifies and classifies road intersections,
//! including complex and clustered intersections.

use oops_core::analysis::*;
use oops_core::data::OsmData;
use std::collections::HashMap;

/// Detector for intersection quality issues
pub struct IntersectionDetector {
    min_degree: usize,
    report_complex: bool,
}

impl IntersectionDetector {
    pub fn new(min_degree: usize, report_complex: bool) -> Self {
        Self { min_degree, report_complex }
    }
}

impl ProblemDetector for IntersectionDetector {
    fn detect_problems(
        &self,
        context: &EnrichedContext,
        _data: &OsmData,
    ) -> Vec<Finding> {
        let mut findings = Vec::new();
        
        // Extract intersection metadata from context
        let degree = context.metadata
            .get("degree")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
            
        let unknown_type = "unknown".to_string();
        let intersection_type = context.metadata
            .get("intersection_type")
            .unwrap_or(&unknown_type);
            
        let cluster_size = context.metadata
            .get("cluster_size")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1);
        
        // Report intersections that meet criteria
        if degree >= self.min_degree {
            let severity = match intersection_type.as_str() {
                "roundabout" => Severity::Info,
                "complex" => if self.report_complex { Severity::Low } else { return findings; },
                "4-way" => Severity::Info,
                "3-way" => Severity::Info,
                _ => Severity::Info,
            };
            
            let oneway_departing_count = context.metadata
                .get("oneway_departing_count")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            
            let oneway_arriving_count = context.metadata
                .get("oneway_arriving_count")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            
            let message = if cluster_size > 1 {
                if oneway_departing_count > 0 || oneway_arriving_count > 0 {
                    format!(
                        "{} intersection with {} connected ways (clustered from {} nodes, {} departing one-ways, {} arriving one-ways)",
                        intersection_type, degree, cluster_size, oneway_departing_count, oneway_arriving_count
                    )
                } else {
                    format!(
                        "{} intersection with {} connected ways (clustered from {} nodes)",
                        intersection_type, degree, cluster_size
                    )
                }
            } else {
                if oneway_departing_count > 0 || oneway_arriving_count > 0 {
                    format!(
                        "{} intersection with {} connected ways ({} departing one-ways, {} arriving one-ways)",
                        intersection_type, degree, oneway_departing_count, oneway_arriving_count
                    )
                } else {
                    format!(
                        "{} intersection with {} connected ways",
                        intersection_type, degree
                    )
                }
            };
            
            let mut finding_context = HashMap::new();
            finding_context.insert("degree".to_string(), degree.to_string());
            finding_context.insert("intersection_type".to_string(), intersection_type.clone());
            finding_context.insert("cluster_size".to_string(), cluster_size.to_string());
            
            if let Some(node_cluster) = context.metadata.get("node_cluster") {
                finding_context.insert("node_cluster".to_string(), node_cluster.clone());
            }
            
            if let Some(connected_ways) = context.metadata.get("connected_ways") {
                finding_context.insert("connected_ways".to_string(), connected_ways.clone());
            }
            
            // Add one-way road information
            if let Some(oneway_departing) = context.metadata.get("oneway_departing_count") {
                finding_context.insert("oneway_departing_count".to_string(), oneway_departing.clone());
            }
            if let Some(oneway_arriving) = context.metadata.get("oneway_arriving_count") {
                finding_context.insert("oneway_arriving_count".to_string(), oneway_arriving.clone());
            }
            if let Some(oneway_ways_departing) = context.metadata.get("oneway_ways_departing") {
                finding_context.insert("oneway_ways_departing".to_string(), oneway_ways_departing.clone());
            }
            if let Some(oneway_ways_arriving) = context.metadata.get("oneway_ways_arriving") {
                finding_context.insert("oneway_ways_arriving".to_string(), oneway_ways_arriving.clone());
            }
            
            // Add roundabout information
            if let Some(roundabout_ways) = context.metadata.get("roundabout_ways") {
                finding_context.insert("roundabout_ways".to_string(), roundabout_ways.clone());
            }
            
            findings.push(Finding {
                category: ProblemCategory::Info,
                severity,
                message: Some(message),
                context: finding_context,
            });
        }
        
        findings
    }
}