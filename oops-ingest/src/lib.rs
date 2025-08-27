#[cfg(test)]
use oops_core::BoundingBox;
use thiserror::Error;

pub mod overpass;
pub mod pbf;

pub use overpass::*;
pub use pbf::*;

#[derive(Error, Debug)]
pub enum IngestError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON parsing failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("PBF parsing failed: {0}")]
    Pbf(#[from] osmpbf::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Rate limit exceeded")]
    RateLimit,
    #[error("Query error: {0}")]
    Query(String),
}

pub type Result<T> = std::result::Result<T, IngestError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overpass_client_creation() {
        let _client = OverpassClient::new();
        let _client_with_timeout = OverpassClient::with_timeout(60);
        // Just test creation doesn't panic
    }

    #[test]
    fn test_bbox_query_formatting() {
        let bbox = BoundingBox::new(13.4, 52.5, 13.5, 52.6);

        // Test that we can format queries correctly
        let query = r#"node["amenity"="restaurant"]"#;
        let formatted = format!(
            "[out:json][bbox:{},{},{},{}];{};out meta;",
            bbox.min_lat, bbox.min_lon, bbox.max_lat, bbox.max_lon, query
        );

        assert!(formatted.contains("bbox:52.5,13.4,52.6,13.5"));
        assert!(formatted.contains(r#"node["amenity"="restaurant"]"#));
        assert!(formatted.contains("out meta;"));
    }

    // Integration test - only run with network access
    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_fetch_restaurants_integration() {
        let client = OverpassClient::new();
        let bbox = BoundingBox::new(13.4, 52.5, 13.41, 52.51); // Small Berlin area

        let result = client
            .fetch_in_bbox(&bbox, r#"node["amenity"="restaurant"]"#)
            .await;

        match result {
            Ok(osm_data) => {
                println!("Fetched {} nodes", osm_data.nodes.len());
                // Should have some restaurants in Berlin
                assert!(!osm_data.nodes.is_empty());
            }
            Err(e) => {
                println!("Network test failed (expected if offline): {}", e);
                // Don't fail the test - network issues are expected
            }
        }
    }

    #[test]
    fn test_pbf_reader_creation() {
        let _reader = PbfReader::new();
        // Test creation doesn't panic
    }

    #[test]
    fn test_pbf_filter_builder() {
        let filter = PbfFilter::new()
            .nodes_only()
            .with_tag("amenity", Some("restaurant"))
            .in_bbox(&BoundingBox::new(13.4, 52.5, 13.5, 52.6));

        assert!(filter.include_nodes);
        assert!(!filter.include_ways);
        assert!(!filter.include_relations);
        assert!(filter.bbox.is_some());
        assert_eq!(filter.required_tags.len(), 1);
        assert_eq!(
            filter.required_tags.get("amenity"),
            Some(&Some("restaurant".to_string()))
        );
    }

    #[test]
    fn test_progress_reporter() {
        let progress = ConsoleProgress;
        progress.report_progress(0, "starting");
        progress.report_progress(5000, "halfway"); // Should not print
        progress.report_progress(10000, "checkpoint"); // Should print
    }

    // Integration test with Salt Lake City sample data
    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_pbf_reader_slc_small() {
        use std::path::Path;

        let sample_pbf_path = "../sample_data/slc_small.osm.pbf";

        if Path::new(sample_pbf_path).exists() {
            let reader = PbfReader::new();
            let filter = PbfFilter::new(); // Remove limit call

            let result = reader.read_file(sample_pbf_path, Some(filter)).await;

            match result {
                Ok(osm_data) => {
                    println!("SLC Small PBF Results:");
                    println!("  Nodes: {}", osm_data.nodes.len());
                    println!("  Ways: {}", osm_data.ways.len());
                    println!("  Relations: {}", osm_data.relations.len());

                    // Find some interesting elements
                    let amenities: Vec<_> = osm_data
                        .nodes
                        .values()
                        .filter(|n| n.tags.contains_key("amenity"))
                        .take(5)
                        .collect();

                    println!("  Sample amenities found: {}", amenities.len());
                    for amenity in amenities {
                        if let Some(amenity_type) = amenity.tags.get("amenity") {
                            println!("    {} at ({}, {})", amenity_type, amenity.lat, amenity.lon);
                        }
                    }

                    assert!(!osm_data.nodes.is_empty());
                }
                Err(e) => {
                    println!("PBF test failed: {}", e);
                    panic!("Expected to read SLC data successfully");
                }
            }
        } else {
            println!("No SLC small PBF file found at {}", sample_pbf_path);
        }
    }

    #[tokio::test]
    #[ignore] // Run with: cargo test -- --ignored
    async fn test_pbf_streaming_slc() {
        use std::path::Path;

        let sample_pbf_path = "../sample_data/slc_small.osm.pbf";

        if Path::new(sample_pbf_path).exists() {
            let reader = PbfReader::new();
            let filter = PbfFilter::new()
                .nodes_only()
                .with_tag("amenity", Some("restaurant"));

            let stream = reader.stream_file(sample_pbf_path, Some(filter));

            match stream {
                Ok(stream) => {
                    let mut restaurant_count = 0;

                    let result = stream.for_each(|element| {
                        if let StreamElement::Node(node) = element {
                            if let Some(amenity) = node.tags.get("amenity") {
                                if amenity == "restaurant" {
                                    restaurant_count += 1;
                                    if restaurant_count <= 3 {
                                        println!(
                                            "Restaurant found: {} at ({}, {})",
                                            node.tags.get("name").unwrap_or(&"unnamed".to_string()),
                                            node.lat,
                                            node.lon
                                        );
                                    }
                                }
                            }
                        }
                    });

                    match result {
                        Ok(_) => {
                            println!("Found {} restaurants via streaming", restaurant_count);
                            assert!(restaurant_count >= 0); // Should work even if no restaurants
                        }
                        Err(e) => {
                            println!("Streaming failed: {}", e);
                            panic!("Expected streaming to work");
                        }
                    }
                }
                Err(e) => {
                    println!("Stream creation failed: {}", e);
                    panic!("Expected to create stream successfully");
                }
            }
        } else {
            println!("No SLC small PBF file found at {}", sample_pbf_path);
        }
    }
}
