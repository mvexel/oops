# OOPS — OSM Object Problem Scanner

A modular Rust toolkit for analyzing OpenStreetMap data quality issues with extensible analyzers and spatial indexing.

## Features

- Ingest data from Overpass API queries or PBF file parsing
- Fast spatial queries for complex analysis use cases 
- Plugin architecture for custom analyzers
- Streaming parsers for large datasets
- Generic analyzer framework 
- **Real-time progress**: Progress reporting for long-running operations

## Quick Start

### Installation

```bash
git clone https://github.com/mvexel/oops
cd oops
cargo build --release
```

### Basic Usage

#### Analyze a PBF file
```rust
use oops_ingest::{PbfReader, PbfFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let reader = PbfReader::new();
    let filter = PbfFilter::new()
        .nodes_only()
        .with_tag("amenity", Some("restaurant"))
        .limit(100);
    
    let osm_data = reader.read_file("data.osm.pbf", Some(filter)).await?;
    println!("Found {} restaurants", osm_data.nodes.len());
    Ok(())
}
```

#### Query Overpass API
```rust
use oops_ingest::{OverpassClient, BoundingBox};

#[tokio::main] 
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = OverpassClient::new();
    let bbox = BoundingBox::new(-111.9, 40.7, -111.8, 40.8); // Salt Lake City area
    
    let osm_data = client.fetch_in_bbox(&bbox, r#"node["amenity"="restaurant"]"#).await?;
    println!("Found {} restaurants in bbox", osm_data.nodes.len());
    Ok(())
}
```

#### Custom Analysis
```rust
use oops_core::{Analyzer, AnalysisQuery, AnalysisResult, BoundingBox};

struct RestaurantAnalyzer;

impl Analyzer for RestaurantAnalyzer {
    fn name(&self) -> &str { "restaurant_analyzer" }
    
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();
        
        for node in data.nodes_in_bbox(&query.area) {
            if let Some("restaurant") = node.tags.get("amenity").map(|s| s.as_str()) {
                let mut result = AnalysisResult::new(node.id, ElementType::Node);
                
                if !node.tags.contains_key("opening_hours") {
                    result.add_finding(Finding {
                        category: ProblemCategory::Missing,
                        severity: Severity::Medium,
                        message: "Missing opening hours".to_string(),
                        recommendation: Some("Add opening_hours tag".to_string()),
                        context: HashMap::new(),
                    });
                }
                
                results.push(result);
            }
        }
        
        results
    }
}
```

## Architecture

```
oops/
├── oops-core/          # Core data structures and analysis framework
├── oops-ingest/        # Data ingestion (Overpass API, PBF files)
├── oops-spatial/       # Geospatial operations and algorithms
├── oops-analyzers/     # Built-in quality analyzers
└── oops-cli/          # Command line interface
```

### Core Concepts

- **OsmData**: Spatially-indexed container for OSM elements
- **Analyzer**: Trait for implementing quality checks
- **AnalysisQuery**: Spatial query with context and filters
- **AnalysisResult**: Generic result format with severity levels and findings

## Sample Data

The repository includes Salt Lake City sample data in `sample_data/`:
- `slc_small.osm.pbf` - Small test dataset (~664KB)
- `slc_large.osm.pbf` - Larger dataset (~5.8MB)

## Development

### Running Tests

```bash
# Unit tests
cargo test

# Integration tests with sample data
cargo test -- --ignored

# Specific test with output
cargo test test_pbf_reader_slc_small -- --ignored --nocapture
```

### Project Structure

- **oops-core**: Fundamental data types (`OsmNode`, `OsmWay`, `OsmRelation`, `OsmData`)
- **oops-ingest**: Data loading from Overpass API and PBF files
- **oops-spatial**: Spatial operations, buffers, intersections (in progress)
- **oops-analyzers**: Quality analysis implementations (planned)
- **oops-cli**: Command-line interface (planned)

### Plugin System

OOPS has a plugin system so you can develop your own custom analyzers:

#### Creating Custom Analyzers

```rust
use oops_core::{Analyzer, AnalysisQuery, AnalysisResult, *};

struct MyAnalyzer;

impl Analyzer for MyAnalyzer {
    fn name(&self) -> &str { "my_analyzer" }
    fn version(&self) -> &str { "1.0.0" }
    fn description(&self) -> &str { "My custom quality analyzer" }
    
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();
        
        // Your analysis logic here
        for node in data.nodes_in_bbox(&query.area) {
            if let Some("restaurant") = node.tags.get("amenity").map(|s| s.as_str()) {
                let mut result = AnalysisResult::new(node.id, ElementType::Node);
                
                if !node.tags.contains_key("opening_hours") {
                    result.add_finding(Finding {
                        category: ProblemCategory::Missing,
                        severity: Severity::Medium,
                        message: "Missing opening hours".to_string(),
                        recommendation: Some("Add opening_hours tag".to_string()),
                        context: HashMap::new(),
                    });
                }
                
                if !result.findings.is_empty() {
                    results.push(result);
                }
            }
        }
        
        results
    }
}
```

#### Using the Plugin Registry

```rust
use oops_core::AnalyzerRegistry;

// Create registry and register analyzers
let mut registry = AnalyzerRegistry::new();
registry.register(MyAnalyzer);
registry.register(RestaurantAnalyzer);
registry.register(AccessibilityAnalyzer);

// List available analyzers
println!("Available analyzers: {:?}", registry.list());

// Get analyzer info
if let Some(info) = registry.get_info("my_analyzer") {
    println!("{} v{}: {}", info.name, info.version, info.description);
}

// Run specific analyzer
let results = registry.analyze("my_analyzer", &query, &data)?;

// Run all analyzers
let all_results = registry.analyze_all(&query, &data);
```

#### Plugin Examples

See `oops-core/examples/plugin_system_demo.rs` for complete examples of:
- **RestaurantAnalyzer**: Checks restaurant data completeness
- **AccessibilityAnalyzer**: Validates wheelchair accessibility tags  
- **HighwaySafetyAnalyzer**: Analyzes sidewalk information on roads

Run the demo:
```bash
cd oops-core && cargo run --example plugin_system_demo
```

### Testing with Sample Data

The project includes real OpenStreetMap data from Salt Lake City for testing:

```bash
# Test PBF parsing
cargo test test_pbf_reader_slc_small -- --ignored --nocapture

# Test streaming parsing
cargo test test_pbf_streaming_slc -- --ignored --nocapture

# Test Overpass API (requires internet)
cargo test test_fetch_restaurants_integration -- --ignored --nocapture
```

## Contributing

1. Follow existing code patterns and conventions
2. Add tests for new functionality  
3. Use TDD approach when possible
4. Document public APIs
5. Test with sample data before submitting

## License

[Add your license here]

## Dependencies

- `geo` - Geospatial operations
- `rstar` - Spatial indexing
- `osmpbf` - PBF file parsing  
- `reqwest` - HTTP client for Overpass API
- `serde` - Serialization
- `tokio` - Async runtime
