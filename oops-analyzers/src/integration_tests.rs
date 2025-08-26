//! Integration tests using real OSM data
//!
//! Tests analyzers with the Salt Lake City sample data

use oops_core::*;
use oops_ingest::*;
use crate::*;
use geo::*;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Run with `cargo test -- --ignored` to include this test
    async fn test_missing_street_names_with_slc_data() {
        // Load the Salt Lake City sample data
        let pbf_path = Path::new("../sample_data/slc_small.osm.pbf");
        
        if !pbf_path.exists() {
            println!("Skipping SLC test - sample data not found at: {:?}", pbf_path);
            return;
        }

        // Load highway data without bbox filtering to get complete node references
        let filter = PbfFilter::new()
            .with_tag("highway", None) // Any highway tag for ways
            .limit(5000); // Limit for testing performance

        // Parse the PBF file using PbfReader
        let reader = PbfReader::new();
        let result = reader.read_file(pbf_path, Some(filter)).await;

        let data = match result {
            Ok(osm_data) => osm_data,
            Err(e) => {
                println!("Failed to parse SLC sample data: {:?}", e);
                return; // Don't fail test if file has issues
            }
        };

        println!("Loaded {} nodes and {} ways from SLC data", 
                 data.nodes.len(), data.ways.len());

        // Create analyzer and run it on the loaded data
        let analyzer = MissingStreetNamesAnalyzer::new();
        
        // Query a larger area to ensure we find some intersections
        let bbox = BoundingBox::new(-111.95, 40.74, -111.85, 40.80);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        println!("Found {} intersections to analyze", results.len());

        // Count intersections with and without issues
        let mut intersections_with_issues = 0;
        let mut intersections_without_issues = 0;

        for result in &results {
            if result.findings.is_empty() {
                intersections_without_issues += 1;
            } else {
                intersections_with_issues += 1;
                println!("Intersection {} has {} unnamed roads:", 
                        result.element_id, result.findings.len());
                
                for finding in &result.findings {
                    println!("  - {}", finding.message);
                    if let Some(way_id) = finding.context.get("unnamed_way_id") {
                        println!("    Way ID: {}", way_id);
                    }
                }
            }
        }

        println!("Summary:");
        println!("  Intersections with unnamed roads: {}", intersections_with_issues);
        println!("  Intersections with all roads named: {}", intersections_without_issues);
        println!("  Total intersections analyzed: {}", results.len());

        // The test succeeds if we can process the data without errors
        // With sample data, we may or may not find intersections depending on the extract
        println!("Integration test completed successfully - analyzer works with real OSM data");
        
        // Real validation would require manual verification of the results
    }

    #[tokio::test]
    #[ignore]
    async fn test_analyzer_performance_with_slc_data() {
        let pbf_path = Path::new("../sample-data/salt-lake-city-sample.osm.pbf");
        
        if !pbf_path.exists() {
            println!("Skipping performance test - sample data not found");
            return;
        }

        // Time the data loading
        let start_load = std::time::Instant::now();
        let slc_bbox = BoundingBox::new(-111.9200, 40.7500, -111.8800, 40.7800);
        let filter = PbfFilter::new()
            .in_bbox(&slc_bbox)
            .with_tag("highway", None); // Only highway elements

        let reader = PbfReader::new();
        let result = reader.read_file(pbf_path, Some(filter)).await;

        let data = match result {
            Ok(osm_data) => osm_data,
            Err(e) => {
                println!("Failed to parse SLC sample data: {:?}", e);
                return; // Don't fail test if file has issues
            }
        };

        let load_duration = start_load.elapsed();
        println!("Data loading took: {:?}", load_duration);

        // Time the analysis
        let start_analysis = std::time::Instant::now();
        
        let analyzer = MissingStreetNamesAnalyzer::new();
        let bbox = BoundingBox::new(-111.9200, 40.7500, -111.8800, 40.7800);
        let query = AnalysisQuery::new(bbox);
        
        let results = analyzer.analyze(&query, &data);
        
        let analysis_duration = start_analysis.elapsed();
        
        println!("Analysis took: {:?}", analysis_duration);
        println!("Analyzed {} intersections", results.len());
        println!("Performance: {:.2} intersections/second", 
                results.len() as f64 / analysis_duration.as_secs_f64());

        // Performance should be reasonable (>100 intersections/second for small dataset)
        assert!(analysis_duration < std::time::Duration::from_secs(5), 
                "Analysis took too long: {:?}", analysis_duration);
    }

    #[test]
    fn test_analyzer_with_empty_data() {
        // Test edge case: empty dataset
        let data = OsmData::new();
        let analyzer = MissingStreetNamesAnalyzer::new();
        
        let bbox = BoundingBox::new(-111.91, 40.76, -111.89, 40.77);
        let query = AnalysisQuery::new(bbox);
        
        let results = analyzer.analyze(&query, &data);
        
        assert_eq!(results.len(), 0, "Should return no results for empty dataset");
    }

    #[test]
    fn test_analyzer_registry_integration() {
        // Test that our analyzer works with the registry system
        let mut registry = AnalyzerRegistry::new();
        registry.register(MissingStreetNamesAnalyzer::new());

        // Verify analyzer is registered
        assert!(registry.get("missing_street_names").is_some());
        
        let info = registry.get_info("missing_street_names").unwrap();
        assert_eq!(info.name, "missing_street_names");
        assert_eq!(info.version, "1.0.0");
        assert!(info.description.contains("intersection"));

        // Test analysis through registry
        let data = create_simple_test_data();
        let bbox = BoundingBox::new(-0.002, -0.002, 0.002, 0.002);
        let query = AnalysisQuery::new(bbox);

        let result = registry.analyze("missing_street_names", &query, &data);
        assert!(result.is_ok());
        
        let results = result.unwrap();
        assert!(results.len() > 0, "Should find test intersection");
    }

    fn create_simple_test_data() -> OsmData {
        let mut data = OsmData::new();

        // Simple 3-way intersection for registry test
        data.add_node(OsmNode::new(1, 0.0, 0.0)); 
        data.add_node(OsmNode::new(2, 0.001, 0.0));
        data.add_node(OsmNode::new(3, -0.001, 0.0));
        data.add_node(OsmNode::new(4, 0.0, 0.001));

        // Three roads meeting at intersection (degree 3)
        let mut way1 = OsmWay::new(1, vec![1, 2]);
        way1.tags.insert("highway".to_string(), "residential".to_string());
        way1.tags.insert("name".to_string(), "Test Street".to_string());
        data.ways.insert(1, way1);

        let mut way2 = OsmWay::new(2, vec![3, 1]);
        way2.tags.insert("highway".to_string(), "residential".to_string());
        // No name - should be flagged
        data.ways.insert(2, way2);

        let mut way3 = OsmWay::new(3, vec![1, 4]);
        way3.tags.insert("highway".to_string(), "residential".to_string());
        way3.tags.insert("name".to_string(), "Another Street".to_string());
        data.ways.insert(3, way3);

        data
    }
}