# Lesson 5: Error Handling Patterns

**Learning Objectives**:
- Replace generic error types with domain-specific errors
- Master Result propagation with `?` operator
- Use thiserror and anyhow for ergonomic error handling
- Design error hierarchies for complex applications

**Time**: 45 minutes  
**Prerequisites**: Lessons 1-4 completed

## Current Error Handling Issues

### Problem 1: Generic Error Types
```rust
// In oops-cli/src/main.rs
async fn analyze_command(
    // ...
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Generic error type loses important information
}
```

### Problem 2: String-based Errors
```rust
// In oops-core/src/analysis.rs
pub fn analyze(
    &self,
    analyzer_name: &str,
    query: &AnalysisQuery,
    data: &OsmData,
) -> Result<Vec<AnalysisResult>, String> {  // ❌ String errors are hard to handle
    match self.get(analyzer_name) {
        Some(analyzer) => Ok(analyzer.analyze(query, data)),
        None => Err(format!(
            "Analyzer '{}' not found. Available: {}",
            analyzer_name,
            self.list().join(", ")
        )),
    }
}
```

### Problem 3: Error Context Loss
```rust
// In oops-ingest (hypothetical)
let data = reader.read_file(&pbf_path, filter).await?;  // What went wrong?
```

## Understanding Rust Error Handling

### The Error Trait
```rust
use std::error::Error;
use std::fmt;

#[derive(Debug)]
struct CustomError {
    message: String,
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CustomError {}
```

### Result vs Panic
- **Result**: Recoverable errors (file not found, network timeout)
- **Panic**: Programming errors (index out of bounds, unwrap on None)

## Your Tasks

### Task 1: Define Domain-Specific Error Types

Create `oops-core/src/error.rs`:

```rust
//! Error types for the OOPS analysis system

use std::fmt;
use thiserror::Error;

/// Main error type for OOPS operations
#[derive(Error, Debug)]
pub enum OopsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Analysis error: {0}")]
    Analysis(#[from] AnalysisError),
    
    #[error("Data ingestion error: {0}")]
    Ingestion(#[from] IngestionError),
    
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
    
    #[error("Spatial operation error: {0}")]
    Spatial(String),
}

/// Errors that occur during analysis
#[derive(Error, Debug)]
pub enum AnalysisError {
    #[error("Analyzer '{analyzer}' not found. Available analyzers: {available}")]
    AnalyzerNotFound {
        analyzer: String,
        available: Vec<String>,
    },
    
    #[error("Invalid configuration for analyzer '{analyzer}': {reason}")]
    InvalidConfig {
        analyzer: String,
        reason: String,
    },
    
    #[error("Analysis failed for element {element_id}: {reason}")]
    AnalysisFailed {
        element_id: i64,
        reason: String,
    },
    
    #[error("Insufficient data for analysis: {details}")]
    InsufficientData {
        details: String,
    },
}

/// Errors during data loading
#[derive(Error, Debug)]
pub enum IngestionError {
    #[error("Failed to parse PBF file: {path}")]
    PbfParseError {
        path: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    
    #[error("Overpass API error: {status_code} - {message}")]
    OverpassError {
        status_code: u16,
        message: String,
    },
    
    #[error("Network timeout after {seconds}s")]
    NetworkTimeout {
        seconds: u64,
    },
    
    #[error("Invalid bounding box: {reason}")]
    InvalidBoundingBox {
        reason: String,
    },
}

/// Configuration-related errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing required configuration: {key}")]
    MissingConfig {
        key: String,
    },
    
    #[error("Invalid value for '{key}': expected {expected}, got '{actual}'")]
    InvalidValue {
        key: String,
        expected: String,
        actual: String,
    },
    
    #[error("Configuration file not found: {path}")]
    FileNotFound {
        path: String,
    },
}

/// Result type alias for convenience
pub type OopsResult<T> = Result<T, OopsError>;
```

Add to `oops-core/src/lib.rs`:
```rust
pub mod error;
pub use error::*;
```

Add to `oops-core/Cargo.toml`:
```toml
[dependencies]
thiserror = "1.0"
```

### Task 2: Update Analysis Registry

Replace string errors in `oops-core/src/analysis.rs`:

```rust
use crate::error::{AnalysisError, OopsResult};

impl AnalyzerRegistry {
    /// Run analysis with a specific analyzer
    pub fn analyze(
        &self,
        analyzer_name: &str,
        query: &AnalysisQuery,
        data: &crate::data::OsmData,
    ) -> Result<Vec<AnalysisResult>, AnalysisError> {
        match self.get(analyzer_name) {
            Some(analyzer) => {
                // Validate configuration if provided
                if let Some(config) = query.context.parameters.get("config") {
                    // Parse config and validate
                    let parsed_config = AnalyzerConfig::from_str(config)
                        .map_err(|reason| AnalysisError::InvalidConfig {
                            analyzer: analyzer_name.to_string(),
                            reason,
                        })?;
                    
                    analyzer.validate_config(&parsed_config)
                        .map_err(|reason| AnalysisError::InvalidConfig {
                            analyzer: analyzer_name.to_string(),
                            reason,
                        })?;
                }
                
                Ok(analyzer.analyze(query, data))
            }
            None => Err(AnalysisError::AnalyzerNotFound {
                analyzer: analyzer_name.to_string(),
                available: self.list().iter().map(|s| s.to_string()).collect(),
            }),
        }
    }
    
    /// Validate all configurations at startup
    pub fn validate_startup(&self, configs: &std::collections::HashMap<String, AnalyzerConfig>) -> Result<(), AnalysisError> {
        for (name, config) in configs {
            if let Some(analyzer) = self.get(name) {
                analyzer.validate_config(config)
                    .map_err(|reason| AnalysisError::InvalidConfig {
                        analyzer: name.clone(),
                        reason,
                    })?;
            }
        }
        Ok(())
    }
}

// Helper for parsing config from strings
impl AnalyzerConfig {
    pub fn from_str(s: &str) -> Result<Self, String> {
        // Parse key=value pairs from string
        let mut config = AnalyzerConfig::new("temp");
        
        for pair in s.split(',') {
            let parts: Vec<&str> = pair.split('=').collect();
            if parts.len() != 2 {
                return Err(format!("Invalid config format: '{}'", pair));
            }
            config.set(parts[0].trim(), parts[1].trim());
        }
        
        Ok(config)
    }
}
```

### Task 3: Add Context with anyhow

For the CLI layer, use `anyhow` for rich error context:

Add to `oops-cli/Cargo.toml`:
```toml
[dependencies]
anyhow = "1.0"
```

Update `oops-cli/src/main.rs`:

```rust
use anyhow::{Context, Result as AnyhowResult};
use oops_core::{OopsError, AnalysisError};

#[tokio::main]
async fn main() -> AnyhowResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze { file, bbox, analyzer, format, verbose } => {
            analyze_command(file, bbox, analyzer, format, verbose)
                .await
                .context("Analysis command failed")?;
        }
        Commands::List => {
            list_analyzers()
                .context("Failed to list analyzers")?;
        }
    }

    Ok(())
}

async fn analyze_command(
    file: Option<PathBuf>,
    bbox: Option<String>,
    analyzer: Option<String>,
    format: OutputFormat,
    verbose: bool,
) -> AnyhowResult<()> {
    if verbose {
        println!("Loading OSM data...");
    }

    // Parse bounding box with better error context
    let query_bbox = if let Some(bbox_str) = bbox {
        Some(parse_bbox(&bbox_str)
            .with_context(|| format!("Invalid bounding box format: '{}'", bbox_str))?)
    } else {
        None
    };

    // Load data with error context
    let data = if let Some(pbf_path) = file {
        if verbose {
            println!("Reading PBF file: {:?}", pbf_path);
        }
        
        let reader = PbfReader::new();
        let filter = if let Some(bbox) = &query_bbox {
            Some(PbfFilter::new().in_bbox(bbox))
        } else {
            None
        };
        
        reader.read_file(&pbf_path, filter)
            .await
            .with_context(|| format!("Failed to read PBF file: {:?}", pbf_path))?
    } else {
        anyhow::bail!("Either --file or --bbox with Overpass API must be provided");
    };

    if verbose {
        println!("Loaded {} nodes and {} ways", data.nodes.len(), data.ways.len());
    }

    // Set up analyzer registry
    let mut registry = AnalyzerRegistry::new();
    registry.register(MissingStreetNamesAnalyzer::new());
    registry.register(MissingMaxspeedAnalyzer::new());

    // Run analysis with better error handling
    let bbox = query_bbox.unwrap_or_else(|| BoundingBox::new(-180.0, -90.0, 180.0, 90.0));
    let query = AnalysisQuery::new(bbox);

    let results = if let Some(analyzer_name) = analyzer {
        // Run specific analyzer with rich error context
        vec![registry.analyze(&analyzer_name, &query, &data)
            .with_context(|| format!("Failed to run analyzer '{}'", analyzer_name))?]
    } else {
        // Run all analyzers, collecting any errors
        let mut all_results = Vec::new();
        let mut errors = Vec::new();
        
        for name in registry.list() {
            match registry.analyze(name, &query, &data) {
                Ok(results) => all_results.push(results),
                Err(e) => errors.push(format!("{}: {}", name, e)),
            }
        }
        
        if !errors.is_empty() {
            anyhow::bail!("Some analyzers failed:\n{}", errors.join("\n"));
        }
        
        all_results
    };

    // Output results
    output_results(results, format)
        .context("Failed to output results")?;

    Ok(())
}

fn parse_bbox(bbox_str: &str) -> Result<BoundingBox, ConfigError> {
    let parts: Vec<&str> = bbox_str.split(',').collect();
    if parts.len() != 4 {
        return Err(ConfigError::InvalidValue {
            key: "bbox".to_string(),
            expected: "min_lon,min_lat,max_lon,max_lat".to_string(),
            actual: bbox_str.to_string(),
        });
    }

    let min_lon: f64 = parts[0].parse()
        .map_err(|_| ConfigError::InvalidValue {
            key: "min_lon".to_string(),
            expected: "number".to_string(),
            actual: parts[0].to_string(),
        })?;
    
    let min_lat: f64 = parts[1].parse()
        .map_err(|_| ConfigError::InvalidValue {
            key: "min_lat".to_string(),
            expected: "number".to_string(),
            actual: parts[1].to_string(),
        })?;
    
    let max_lon: f64 = parts[2].parse()
        .map_err(|_| ConfigError::InvalidValue {
            key: "max_lon".to_string(),
            expected: "number".to_string(),
            actual: parts[2].to_string(),
        })?;
    
    let max_lat: f64 = parts[3].parse()
        .map_err(|_| ConfigError::InvalidValue {
            key: "max_lat".to_string(),
            expected: "number".to_string(),
            actual: parts[3].to_string(),
        })?;

    Ok(BoundingBox::new(min_lon, min_lat, max_lon, max_lat))
}

fn list_analyzers() -> AnyhowResult<()> {
    let mut registry = AnalyzerRegistry::new();
    registry.register(MissingStreetNamesAnalyzer::new());
    registry.register(MissingMaxspeedAnalyzer::new());

    println!("Available analyzers:");
    for info in registry.get_all_info() {
        println!("  {} (v{}) - {}", info.name, info.version, info.description);
    }
    
    Ok(())
}
```

### Task 4: Custom Error for Analyzers

Update analyzer trait to return domain-specific errors:

```rust
// In oops-core/src/analysis.rs
pub trait Analyzer: Send + Sync {
    // ... existing methods
    
    /// Analyze data and return results or analysis-specific errors
    fn analyze(&self, query: &AnalysisQuery, data: &crate::data::OsmData) -> Result<Vec<AnalysisResult>, AnalysisError>;
}
```

Update `MissingMaxspeedAnalyzer`:

```rust
impl Analyzer for MissingMaxspeedAnalyzer {
    // ... existing methods
    
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Result<Vec<AnalysisResult>, AnalysisError> {
        if data.ways.is_empty() {
            return Err(AnalysisError::InsufficientData {
                details: "No ways found in dataset".to_string(),
            });
        }
        
        let mut results = Vec::new();

        // Existing analysis logic...
        for way in data.ways.values() {
            if !self.should_analyze_way(way) {
                continue;
            }

            if self.has_maxspeed_tag(way) {
                continue;
            }

            // Check if any part of this way is within our query area
            let way_in_query_area = way.nodes.iter().any(|&node_id| {
                if let Some(node) = data.get_node(node_id) {
                    query.area.contains_point(node.lon, node.lat)
                } else {
                    // Log missing node as a warning, but don't fail analysis
                    eprintln!("Warning: Node {} referenced by way {} not found", node_id, way.id);
                    false
                }
            });

            if !way_in_query_area {
                continue;
            }

            // Rest of analysis logic...
            let mut result = AnalysisResult::new(way.id, ElementType::Way);

            let finding = Finding {
                category: ProblemCategory::Missing,
                severity: self.determine_severity(way),
                message: Some(self.create_finding_message(way)),
                recommendation: Some(self.create_recommendation(way)),
                context: self.create_finding_context(way),
            };

            result.add_finding(finding);
            results.push(result);
        }

        results.sort_by_key(|r| r.element_id);
        Ok(results)
    }
}
```

### Task 5: Error Recovery Strategies

Add error recovery for non-critical failures:

```rust
// In analysis registry
impl AnalyzerRegistry {
    /// Run all analyzers with error recovery
    pub fn analyze_all_with_recovery(
        &self,
        query: &AnalysisQuery,
        data: &crate::data::OsmData,
    ) -> (HashMap<String, Vec<AnalysisResult>>, Vec<AnalysisError>) {
        let mut results = HashMap::new();
        let mut errors = Vec::new();
        
        for (name, analyzer) in &self.analyzers {
            match analyzer.analyze(query, data) {
                Ok(analysis_results) => {
                    results.insert(name.clone(), analysis_results);
                }
                Err(e) => {
                    eprintln!("Warning: Analyzer '{}' failed: {}", name, e);
                    errors.push(e);
                    // Continue with other analyzers
                }
            }
        }
        
        (results, errors)
    }
}
```

## Error Handling Best Practices

### 1. Fail Fast for Programming Errors
```rust
// Use panic! for programmer mistakes
if nodes.is_empty() {
    panic!("Nodes vector should never be empty at this point");
}

// Use Result for recoverable errors
fn parse_coordinate(s: &str) -> Result<f64, ConfigError> {
    s.parse().map_err(|_| ConfigError::InvalidValue {
        key: "coordinate".to_string(),
        expected: "floating point number".to_string(),
        actual: s.to_string(),
    })
}
```

### 2. Use ? Operator for Error Propagation
```rust
fn complex_operation() -> OopsResult<String> {
    let data = load_data()?;          // Propagates IngestionError
    let results = analyze(&data)?;     // Propagates AnalysisError
    let formatted = format_output(results)?;  // Propagates any error
    Ok(formatted)
}
```

### 3. Add Context at Application Boundaries
```rust
// Library code - specific errors
pub fn read_pbf_file(path: &Path) -> Result<OsmData, IngestionError> {
    // Implementation
}

// Application code - adds context
let data = read_pbf_file(&path)
    .with_context(|| format!("Failed to process user input file: {}", path.display()))?;
```

### 4. Design Error Hierarchies
```rust
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Connection failed")]
    ConnectionFailed(#[from] std::io::Error),
    
    #[error("Query failed: {query}")]
    QueryFailed { query: String },
    
    #[error("Transaction aborted")]
    TransactionAborted,
}

#[derive(Error, Debug)]
pub enum ApplicationError {
    #[error("Database error")]
    Database(#[from] DatabaseError),
    
    #[error("Validation error")]
    Validation(String),
}
```

## Testing Error Conditions

Create tests for error scenarios:

```rust
#[cfg(test)]
mod error_tests {
    use super::*;
    
    #[test]
    fn test_analyzer_not_found_error() {
        let registry = AnalyzerRegistry::new();
        let query = AnalysisQuery::new(BoundingBox::new(0.0, 0.0, 1.0, 1.0));
        let data = OsmData::new();
        
        let result = registry.analyze("nonexistent", &query, &data);
        
        match result {
            Err(AnalysisError::AnalyzerNotFound { analyzer, available }) => {
                assert_eq!(analyzer, "nonexistent");
                assert!(available.is_empty());
            }
            _ => panic!("Expected AnalyzerNotFound error"),
        }
    }
    
    #[test]
    fn test_invalid_config_error() {
        let analyzer = MissingMaxspeedAnalyzer::new();
        let mut config = AnalyzerConfig::new("missing_maxspeed");
        config.set("include_trunk_links", "maybe");  // Invalid value
        
        let result = analyzer.validate_config(&config);
        assert!(result.is_err());
    }
    
    #[test]
    fn test_error_chain() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let ingestion_error = IngestionError::PbfParseError {
            path: "test.pbf".to_string(),
            source: Box::new(io_error),
        };
        let oops_error = OopsError::Ingestion(ingestion_error);
        
        // Test error chain
        assert!(oops_error.source().is_some());
        assert!(oops_error.source().unwrap().source().is_some());
    }
}
```

## Key Takeaways

1. **Use typed errors instead of strings** - Better error handling and tooling support
2. **thiserror for library errors** - Derive Error trait with rich context
3. **anyhow for application errors** - Easy error chaining and context
4. **Fail fast for programming errors** - Use panic! for invariant violations
5. **Error hierarchies model your domain** - Specific error types for specific problems
6. **Add context at boundaries** - Help users understand what went wrong
7. **Test error conditions** - Ensure error handling works correctly

## Next Steps

With robust error handling in place, [Lesson 6: Async Programming](rust-lesson-06-async.md) will explore making the system asynchronous for better performance with large datasets.

## Common Error Handling Patterns

### Error Conversion Chain
```rust
// Automatic conversion through From trait
fn operation() -> Result<(), OopsError> {
    std::fs::read("file.txt")?;  // io::Error -> OopsError
    Ok(())
}
```

### Custom Error Context
```rust
// Manual context addition
.map_err(|e| AnalysisError::AnalysisFailed {
    element_id: node.id,
    reason: format!("Geometric calculation failed: {}", e),
})
```

### Result Extensions
```rust
use anyhow::Context;

result.context("While processing intersection data")?;
result.with_context(|| format!("Processing node {}", node_id))?;
```