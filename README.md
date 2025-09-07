# OOPS — OSM Object Problem Scanner

A modular Rust toolkit for analyzing OpenStreetMap data quality issues with extensible analyzers and spatial indexing.

## Features

- **Command Line Interface**: Full-featured CLI with multiple output formats
- **Data Ingestion**: Overpass API queries and PBF file parsing
- **Spatial Analysis**: Fast intersection detection and spatial queries using the geo ecosystem
- **Plugin Architecture**: Extensible analyzer system with registry
- **Real Data Validation**: Integration tested with Salt Lake City OSM data
- **Multiple Output Formats**: JSON, CSV, and human-readable summary formats
- **Streaming Parsers**: Memory-efficient processing of large datasets

## Quick Start

### Installation

```bash
git clone https://github.com/mvexel/oops
cd oops
cargo build --release
```

### Command Line Usage

```bash
# List available analyzers
cargo run --bin oops-cli -- list

# Analyze a PBF file
cargo run --bin oops-cli -- analyze --file data.osm.pbf --verbose

# Different output formats
cargo run --bin oops-cli -- analyze --file data.osm.pbf --format summary
cargo run --bin oops-cli -- analyze --file data.osm.pbf --format csv
cargo run --bin oops-cli -- analyze --file data.osm.pbf --format json

# Run specific analyzer
cargo run --bin oops-cli -- analyze --file data.osm.pbf --analyzer missing_maxspeed
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
                        message: "Missing opening hours".into(),
                        recommendation: Some("Add opening_hours tag".into()),
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

- **oops-core**: Fundamental data types (`OsmNode`, `OsmWay`, `OsmRelation`, `OsmData`) + analyzer framework
- **oops-ingest**: Data loading from Overpass API and PBF files  
- **oops-analyzers**: Quality analysis implementations using composable architecture
- **oops-cli**: Command-line interface with multiple output formats

### Plugin System

OOPS has a plugin system so you can develop your own custom analyzers:

#### Creating Custom Analyzers

```rust
use oops_core::analysis::*;

// Create a composable analyzer for missing opening hours on restaurants
pub fn build_restaurant_opening_hours_analyzer() -> Analyzer {
    Analyzer::new(
        "restaurant_opening_hours",
        "1.0.0",
        "Finds restaurants missing opening hours",
        
        // Select restaurant nodes
        Box::new(NodeSelector::new()
            .has_tag("amenity", "restaurant")),
            
        // No additional context needed for this simple analyzer
        Box::new(NoContextProvider::new()),
        
        // Detect missing opening_hours tag
        Box::new(MissingTagDetector::new("opening_hours")
            .with_severity(Severity::Medium)
            .with_message_template("Restaurant missing opening hours")
            .with_recommendation_template("Add opening_hours tag to indicate when the restaurant is open"))
    )
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

#### Built-in Analyzers

The `oops-analyzers` crate provides several built-in analyzers to identify common data quality issues.

##### `missing_maxspeed`

-   **Purpose**: Identifies major roads that are missing a `maxspeed` tag.
-   **How it works**:
    1.  **Selector**: It selects OSM `way` elements that are tagged with `highway` values such as `motorway`, `trunk`, `primary`, `secondary`, or `tertiary`.
    2.  **Detector**: For each selected way, it checks for the existence of the `maxspeed` tag.
    3.  **Finding**: If a `maxspeed` tag is not present, it generates a finding, flagging the way as incomplete.

##### `pedestrian_completeness`

-   **Purpose**: Analyzes the coverage of sidewalks along roads to assess pedestrian infrastructure completeness.
-   **How it works**:
    1.  **Selector**: It selects OSM `way` elements representing roads where pedestrians are common (e.g., `highway=residential`, `highway=tertiary`).
    2.  **Context Provider**: It uses a spatial context provider to find nearby OSM ways within a certain buffer distance of the selected road.
    3.  **Detector**: It analyzes the nearby ways to determine if they are sidewalks (`highway=footway` and `footway=sidewalk`) and assesses whether the road has adequate sidewalk coverage.
    4.  **Finding**: If sidewalk coverage is insufficient, it generates a finding.

#### Plugin Examples

See `oops-core/examples/plugin_system_demo.rs` for complete examples of the composable architecture with:
- **Missing maxspeed analyzer**: Uses generic components with custom detector
- **Pedestrian completeness analyzer**: Uses spatial context for sidewalk analysis

Run the demo:
```bash
cd oops-core && cargo run --example plugin_system_demo
```

### Testing with Sample Data

The project includes real OpenStreetMap data for testing:

```bash
# Test PBF parsing
cargo test test_pbf_reader_slc_small -- --ignored --nocapture

# Test streaming parsing
cargo test test_pbf_streaming_slc -- --ignored --nocapture

# Test Overpass API (requires internet)
cargo test test_fetch_restaurants_integration -- --ignored --nocapture

# Run analyzer registry integration
cargo test test_analyzer_registry_integration
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
