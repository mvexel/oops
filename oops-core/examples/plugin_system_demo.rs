//! Plugin System Demo
//!
//! This example demonstrates how to create and use custom analyzers
//! with the OOPS plugin system.
//!
//! Usage: cargo run --example plugin_system_demo

use oops_core::{
    AnalysisContext, AnalysisQuery, AnalysisResult, Analyzer, AnalyzerRegistry, BoundingBox,
    ElementType, Finding, OsmData, OsmNode, ProblemCategory, Severity,
};
use std::collections::HashMap;

/// Example 1: Restaurant Quality Analyzer
struct RestaurantAnalyzer;

impl Analyzer for RestaurantAnalyzer {
    fn name(&self) -> &str {
        "restaurant_quality"
    }

    fn version(&self) -> &str {
        "1.2.0"
    }

    fn description(&self) -> &str {
        "Analyzes restaurant data completeness and quality"
    }

    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();

        for node in data.nodes_in_bbox(&query.area) {
            if let Some("restaurant") = node.tags.get("amenity").map(|s| s.as_str()) {
                let mut result = AnalysisResult::new(node.id, ElementType::Node);

                // Check for required information
                let required_tags = ["opening_hours", "cuisine", "name"];
                for tag in &required_tags {
                    if !node.tags.contains_key(*tag) {
                        result.add_finding(Finding {
                            category: ProblemCategory::Missing,
                            severity: if *tag == "name" {
                                Severity::High
                            } else {
                                Severity::Medium
                            },
                            message: Some(format!("Missing {} information", tag.replace("_", " "))),
                            recommendation: Some(format!("Add {}=* tag", tag)),
                            context: HashMap::new(),
                        });
                    }
                }

                if !result.findings.is_empty() {
                    results.push(result);
                }
            }
        }

        results
    }
}

/// Example 2: Accessibility Analyzer
struct AccessibilityAnalyzer;

impl Analyzer for AccessibilityAnalyzer {
    fn name(&self) -> &str {
        "accessibility_checker"
    }

    fn description(&self) -> &str {
        "Checks for wheelchair accessibility information on public amenities"
    }

    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();
        let public_amenities = ["restaurant", "cafe", "shop", "bank", "pharmacy", "hospital"];

        for node in data.nodes_in_bbox(&query.area) {
            if let Some(amenity) = node.tags.get("amenity") {
                if public_amenities.contains(&amenity.as_str()) {
                    let mut result = AnalysisResult::new(node.id, ElementType::Node);

                    if !node.tags.contains_key("wheelchair") {
                        result.add_finding(Finding {
                            category: ProblemCategory::Missing,
                            severity: Severity::Medium,
                            message: Some("Missing wheelchair accessibility information".into()),
                            recommendation: Some("Add wheelchair=yes/no/limited tag".into()),
                            context: {
                                let mut ctx = HashMap::new();
                                ctx.insert("amenity".into(), amenity.clone());
                                ctx
                            },
                        });
                    }

                    if !result.findings.is_empty() {
                        results.push(result);
                    }
                }
            }
        }

        results
    }
}

/// Example 3: Highway Safety Analyzer
struct HighwaySafetyAnalyzer;

impl Analyzer for HighwaySafetyAnalyzer {
    fn name(&self) -> &str {
        "highway_safety"
    }

    fn version(&self) -> &str {
        "0.9.0"
    }

    fn description(&self) -> &str {
        "Analyzes highway safety features like sidewalks and crossings"
    }

    fn analyze(&self, _query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();

        // Analyze ways (roads) for safety features
        for way in data.ways.values() {
            if let Some(highway) = way.tags.get("highway") {
                // Focus on roads that should have sidewalks
                let needs_sidewalks =
                    ["primary", "secondary", "tertiary", "residential"].contains(&highway.as_str());

                if needs_sidewalks {
                    let mut result = AnalysisResult::new(way.id, ElementType::Way);

                    if !way.tags.contains_key("sidewalk") {
                        result.add_finding(Finding {
                            category: ProblemCategory::Missing,
                            severity: match highway.as_str() {
                                "primary" | "secondary" => Severity::High,
                                _ => Severity::Medium,
                            },
                            message: Some(
                                format!("Missing sidewalk information on {} road", highway),
                            ),
                            recommendation: Some("Add sidewalk=both/left/right/no tag".into()),
                            context: {
                                let mut ctx = HashMap::new();
                                ctx.insert("highway".into(), highway.clone());
                                ctx
                            },
                        });
                    }

                    if !result.findings.is_empty() {
                        results.push(result);
                    }
                }
            }
        }

        results
    }
}

fn main() {
    println!("🔌 OOPS Plugin System Demo");
    println!("===========================");

    // Create registry and register analyzers
    let mut registry = AnalyzerRegistry::new();

    println!("📝 Registering analyzers...");
    registry.register(Box::new(RestaurantAnalyzer));
    registry.register(Box::new(AccessibilityAnalyzer));
    registry.register(Box::new(HighwaySafetyAnalyzer));

    // List all registered analyzers
    println!("\n📋 Registered analyzers:");
    for info in registry.get_all_info() {
        println!("  • {} v{}: {}", info.name, info.version, info.description);
    }

    // Create some sample data
    let mut data = OsmData::new();

    // Add a restaurant without complete information
    let mut restaurant_tags = HashMap::new();
    restaurant_tags.insert("amenity".into(), "restaurant".into());
    restaurant_tags.insert("name".into(), "Test Restaurant".into());
    // Missing: opening_hours, cuisine

    let restaurant = OsmNode {
        id: 1001,
        lat: 40.7500,
        lon: -111.8900,
        tags: restaurant_tags,
    };
    data.add_node(restaurant);

    // Create analysis query
    let bbox = BoundingBox::new(-111.95, 40.70, -111.85, 40.80);
    let query = AnalysisQuery {
        area: bbox,
        elements: Vec::new(),
        context: AnalysisContext::default(),
    };

    println!("\n🔍 Running individual analyzer:");
    match registry.analyze("restaurant_quality", &query, &data) {
        Ok(results) => {
            println!("  Found {} issues with restaurants", results.len());
            for result in results {
                println!(
                    "    • Restaurant ID {}: {} findings",
                    result.element_id,
                    result.findings.len()
                );
                for finding in result.findings {
                    println!("      - {:?}", finding.message);
                }
            }
        }
        Err(e) => println!("  Error: {}", e),
    }

    println!("\n🎯 Running all analyzers:");
    let all_results = registry.analyze_all(&query, &data);

    let mut total_findings = 0;
    for (analyzer_name, results) in all_results {
        let findings_count: usize = results.iter().map(|r| r.findings.len()).sum();
        total_findings += findings_count;

        println!(
            "  {}: {} elements analyzed, {} findings",
            analyzer_name,
            results.len(),
            findings_count
        );
    }

    println!("\n✨ Plugin system demo complete!");
    println!("  Total findings across all analyzers: {}", total_findings);
    println!("\n💡 To create your own analyzer:");
    println!("  1. Implement the Analyzer trait");
    println!("  2. Register with registry.register(YourAnalyzer)");
    println!("  3. Run analysis with registry.analyze(name, query, data)");
}
