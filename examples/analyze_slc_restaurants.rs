//! Example: Analyze restaurant quality in Salt Lake City
//! 
//! This example demonstrates:
//! - Loading PBF data with filtering
//! - Creating a custom analyzer
//! - Running analysis with spatial queries
//! - Reporting results with severity levels
//! 
//! Usage: cargo run --example analyze_slc_restaurants

use oops_core::{
    Analyzer, AnalysisQuery, AnalysisResult, BoundingBox, ElementType, 
    Finding, ProblemCategory, Severity, AnalysisContext
};
use oops_ingest::{PbfReader, PbfFilter};
use std::collections::HashMap;

struct RestaurantQualityAnalyzer;

impl Analyzer for RestaurantQualityAnalyzer {
    fn name(&self) -> &str {
        "restaurant_quality"
    }
    
    fn analyze(&self, query: &AnalysisQuery, data: &oops_core::OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();
        
        // Find all restaurants in the query area
        for node in data.nodes_in_bbox(&query.area) {
            if let Some("restaurant") = node.tags.get("amenity").map(|s| s.as_str()) {
                let mut result = AnalysisResult::new(node.id, ElementType::Node);
                
                // Check for missing information
                if !node.tags.contains_key("opening_hours") {
                    result.add_finding(Finding {
                        category: ProblemCategory::Missing,
                        severity: Severity::Medium,
                        message: "Missing opening hours information".into(),
                        recommendation: Some("Add opening_hours=* tag with business hours".into()),
                        context: {
                            let mut ctx = HashMap::new();
                            ctx.insert("name".into(), 
                                node.tags.get("name").unwrap_or(&"unnamed".into()).clone());
                            ctx
                        },
                    });
                }
                
                if !node.tags.contains_key("cuisine") {
                    result.add_finding(Finding {
                        category: ProblemCategory::Missing,
                        severity: Severity::Low,
                        message: "Missing cuisine information".into(),
                        recommendation: Some("Add cuisine=* tag (e.g., cuisine=pizza)".into()),
                        context: HashMap::new(),
                    });
                }
                
                if !node.tags.contains_key("phone") && !node.tags.contains_key("contact:phone") {
                    result.add_finding(Finding {
                        category: ProblemCategory::Missing,
                        severity: Severity::Low,
                        message: "Missing contact information".into(),
                        recommendation: Some("Add phone=* or contact:phone=* tag".into()),
                        context: HashMap::new(),
                    });
                }
                
                // Only add results that have findings
                if !result.findings.is_empty() {
                    results.push(result);
                }
            }
        }
        
        results
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🍴 OOPS Restaurant Quality Analysis - Salt Lake City");
    println!("==================================================");
    
    // Load restaurant data from Salt Lake City PBF
    println!("📁 Loading Salt Lake City data...");
    let reader = PbfReader::new();
    let filter = PbfFilter::new()
        .nodes_only()
        .with_tag("amenity", Some("restaurant"));
    
    let osm_data = reader.read_file("sample_data/slc_small.osm.pbf", Some(filter)).await?;
    println!("✅ Loaded {} restaurants", osm_data.nodes.len());
    
    // Define analysis area (Salt Lake City downtown area)
    let analysis_bbox = BoundingBox::new(-111.95, 40.72, -111.85, 40.78);
    
    // Create analysis query
    let query = AnalysisQuery {
        area: analysis_bbox,
        elements: Vec::new(), // Analyze all restaurants in area
        context: AnalysisContext::default(),
    };
    
    // Run the analysis
    println!("🔍 Running restaurant quality analysis...");
    let analyzer = RestaurantQualityAnalyzer;
    let results = analyzer.analyze(&query, &osm_data);
    
    // Report results
    println!("\n📊 Analysis Results:");
    println!("===================");
    
    if results.is_empty() {
        println!("🎉 No quality issues found!");
        return Ok(());
    }
    
    // Group by severity
    let mut critical = 0;
    let mut high = 0;
    let mut medium = 0;
    let mut low = 0;
    
    for result in &results {
        match result.overall_severity {
            Severity::Critical => critical += 1,
            Severity::High => high += 1,
            Severity::Medium => medium += 1,
            Severity::Low => low += 1,
            Severity::Info => {},
        }
    }
    
    println!("📈 Summary by Severity:");
    if critical > 0 { println!("  🔴 Critical: {}", critical); }
    if high > 0 { println!("  🟠 High: {}", high); }
    if medium > 0 { println!("  🟡 Medium: {}", medium); }
    if low > 0 { println!("  🟢 Low: {}", low); }
    
    println!("\n🔍 Detailed Findings:");
    for (i, result) in results.iter().take(10).enumerate() {
        let node = osm_data.nodes.get(&result.element_id).unwrap();
        let name = node.tags.get("name").unwrap_or(&"Unnamed Restaurant".into());
        
        println!("\n{}. {} (ID: {})", i + 1, name, result.element_id);
        println!("   📍 Location: {:.5}, {:.5}", node.lat, node.lon);
        println!("   ⚠️  Severity: {:?}", result.overall_severity);
        
        for finding in &result.findings {
            println!("   • {}", finding.message);
            if let Some(rec) = &finding.recommendation {
                println!("     💡 {}", rec);
            }
        }
    }
    
    if results.len() > 10 {
        println!("\n... and {} more restaurants with issues", results.len() - 10);
    }
    
    println!("\n✨ Analysis complete! Consider contributing fixes to OpenStreetMap.");
    
    Ok(())
}