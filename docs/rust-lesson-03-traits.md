# Lesson 3: Trait System & Plugins

**Learning Objectives**:
- Master trait definitions and implementations  
- Understand trait objects and dynamic dispatch
- Build a plugin system with the registry pattern
- Practice generic programming concepts

**Time**: 60 minutes  
**Prerequisites**: Lessons 1-2 completed

## Understanding the Analyzer Trait

The core of OOPS's extensibility is the `Analyzer` trait:

```rust
pub trait Analyzer: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str { "1.0.0" }  // Default implementation
    fn description(&self) -> &str;
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult>;
}
```

**Key concepts**:
- **Trait**: Interface that types can implement
- **Default methods**: `version()` has a default implementation
- **Trait bounds**: `Send + Sync` enable thread safety
- **Associated functions**: Methods that operate on `self`

## Current Implementation Analysis

### Strengths
```rust
// Clean interface
impl Analyzer for MissingMaxspeedAnalyzer {
    fn name(&self) -> &str { "missing_maxspeed" }
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        // Implementation
    }
}

// Easy registration
registry.register(MissingMaxspeedAnalyzer::new());
```

### Areas for Improvement
1. **No configuration validation** - Analyzers can't report invalid config
2. **No capabilities reporting** - Can't query what an analyzer supports  
3. **Synchronous only** - No async analysis support
4. **Limited metadata** - Only name/version/description

## Your Tasks

### Task 1: Enhance the Analyzer Trait

Add these methods to the `Analyzer` trait in `oops-core/src/analysis.rs`:

```rust
pub trait Analyzer: Send + Sync {
    // Existing methods...
    
    /// Report what types of OSM elements this analyzer processes
    fn supported_elements(&self) -> Vec<ElementType> {
        vec![ElementType::Node, ElementType::Way, ElementType::Relation]
    }
    
    /// Validate configuration before analysis
    fn validate_config(&self, config: &AnalyzerConfig) -> Result<(), String> {
        Ok(()) // Default: accept any config
    }
    
    /// Estimate analysis complexity (for progress reporting)
    fn estimate_complexity(&self, data: &OsmData) -> usize {
        data.nodes.len() + data.ways.len() + data.relations.len()
    }
}
```

### Task 2: Implement Enhanced Methods

Update `MissingMaxspeedAnalyzer` to override these methods:

```rust
impl Analyzer for MissingMaxspeedAnalyzer {
    // ... existing methods
    
    fn supported_elements(&self) -> Vec<ElementType> {
        vec![ElementType::Way] // Only analyzes ways
    }
    
    fn validate_config(&self, config: &AnalyzerConfig) -> Result<(), String> {
        // Validate link road configuration
        if let Some(value) = config.get("include_trunk_links") {
            if value != "true" && value != "false" {
                return Err("include_trunk_links must be 'true' or 'false'".to_string());
            }
        }
        Ok(())
    }
    
    fn estimate_complexity(&self, data: &OsmData) -> usize {
        data.ways.len() // Only processes ways
    }
}
```

### Task 3: Create a New Analyzer

Build a simple analyzer to practice trait implementation:

Create `oops-analyzers/src/duplicate_nodes.rs`:

```rust
//! Duplicate Nodes Analyzer
//! 
//! Finds nodes that are very close to each other (potential duplicates)

use oops_core::*;
use std::collections::HashMap;

pub struct DuplicateNodesAnalyzer {
    config: AnalyzerConfig,
}

impl DuplicateNodesAnalyzer {
    pub fn new() -> Self {
        Self {
            config: AnalyzerConfig::new("duplicate_nodes"),
        }
    }
    
    pub fn with_config(config: AnalyzerConfig) -> Self {
        Self { config }
    }
    
    fn get_threshold_meters(&self) -> f64 {
        self.config
            .get("threshold_meters")
            .and_then(|s| s.parse().ok())
            .unwrap_or(5.0) // Default 5 meter threshold
    }
}

impl Analyzer for DuplicateNodesAnalyzer {
    fn name(&self) -> &str {
        "duplicate_nodes"
    }
    
    fn description(&self) -> &str {
        "Finds nodes that are suspiciously close to each other (potential duplicates)"
    }
    
    fn supported_elements(&self) -> Vec<ElementType> {
        vec![ElementType::Node]
    }
    
    fn validate_config(&self, config: &AnalyzerConfig) -> Result<(), String> {
        if let Some(threshold) = config.get("threshold_meters") {
            match threshold.parse::<f64>() {
                Ok(t) if t > 0.0 => Ok(()),
                Ok(_) => Err("threshold_meters must be positive".to_string()),
                Err(_) => Err("threshold_meters must be a valid number".to_string()),
            }
        } else {
            Ok(())
        }
    }
    
    fn estimate_complexity(&self, data: &OsmData) -> usize {
        // O(n²) comparison, but we can optimize with spatial indexing
        data.nodes.len().pow(2)
    }
    
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let threshold = self.get_threshold_meters();
        let mut results = Vec::new();
        let mut processed = std::collections::HashSet::new();
        
        // Get nodes in the query area
        let nodes_in_area: Vec<_> = data.nodes_in_bbox(&query.area);
        
        for (i, node1) in nodes_in_area.iter().enumerate() {
            if processed.contains(&node1.id) {
                continue;
            }
            
            let point1 = node1.to_point();
            let mut duplicates = Vec::new();
            
            // Check against remaining nodes
            for node2 in &nodes_in_area[i+1..] {
                if processed.contains(&node2.id) {
                    continue;
                }
                
                let point2 = node2.to_point();
                let distance = point1.haversine_distance(&point2);
                
                if distance <= threshold {
                    duplicates.push(node2.id);
                    processed.insert(node2.id);
                }
            }
            
            if !duplicates.is_empty() {
                let mut result = AnalysisResult::new(node1.id, ElementType::Node);
                
                let mut context = HashMap::new();
                context.insert("duplicate_ids".to_string(), 
                              duplicates.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","));
                context.insert("threshold_meters".to_string(), threshold.to_string());
                
                let finding = Finding {
                    category: ProblemCategory::Invalid,
                    severity: if duplicates.len() > 2 { Severity::High } else { Severity::Medium },
                    message: Some(format!(
                        "Node {} has {} potential duplicates within {}m",
                        node1.id, duplicates.len(), threshold
                    )),
                    recommendation: Some(
                        "Review these nodes for potential merging or deletion".to_string()
                    ),
                    context,
                };
                
                result.add_finding(finding);
                results.push(result);
                processed.insert(node1.id);
            }
        }
        
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_duplicate_nodes_analyzer() {
        let analyzer = DuplicateNodesAnalyzer::new();
        
        // Test metadata
        assert_eq!(analyzer.name(), "duplicate_nodes");
        assert_eq!(analyzer.supported_elements(), vec![ElementType::Node]);
        
        // Test config validation
        let mut valid_config = AnalyzerConfig::new("duplicate_nodes");
        valid_config.set("threshold_meters", "10.0");
        assert!(analyzer.validate_config(&valid_config).is_ok());
        
        let mut invalid_config = AnalyzerConfig::new("duplicate_nodes");
        invalid_config.set("threshold_meters", "-5.0");
        assert!(analyzer.validate_config(&invalid_config).is_err());
    }
}
```

Don't forget to add the module to `oops-analyzers/src/lib.rs`:
```rust
pub mod duplicate_nodes;
pub use duplicate_nodes::DuplicateNodesAnalyzer;
```

### Task 4: Registry Improvements

Enhance the `AnalyzerRegistry` to use the new trait methods:

```rust
impl AnalyzerRegistry {
    /// Get detailed information about an analyzer
    pub fn get_detailed_info(&self, name: &str) -> Option<DetailedAnalyzerInfo> {
        self.get(name).map(|analyzer| {
            let test_data = crate::data::OsmData::new(); // Empty data for complexity estimation
            
            DetailedAnalyzerInfo {
                name: analyzer.name().to_string(),
                version: analyzer.version().to_string(),
                description: analyzer.description().to_string(),
                supported_elements: analyzer.supported_elements(),
                estimated_complexity: analyzer.estimate_complexity(&test_data),
            }
        })
    }
    
    /// Validate configuration for all registered analyzers
    pub fn validate_all_configs(&self, configs: &HashMap<String, AnalyzerConfig>) -> Vec<String> {
        let mut errors = Vec::new();
        
        for (name, analyzer) in &self.analyzers {
            if let Some(config) = configs.get(name) {
                if let Err(error) = analyzer.validate_config(config) {
                    errors.push(format!("{}: {}", name, error));
                }
            }
        }
        
        errors
    }
}

#[derive(Debug, Clone)]
pub struct DetailedAnalyzerInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub supported_elements: Vec<ElementType>,
    pub estimated_complexity: usize,
}
```

## Advanced Trait Concepts

### 1. Trait Objects vs Generics

**Current approach (Trait objects)**:
```rust
Box<dyn Analyzer>  // Runtime polymorphism, heap allocation
```

**Alternative (Generics)**:
```rust
fn analyze_with<T: Analyzer>(analyzer: T, data: &OsmData) -> Vec<AnalysisResult> {
    // Compile-time polymorphism, stack allocation
}
```

**Trade-offs**:
- Trait objects: Runtime flexibility, collection of different types
- Generics: Compile-time optimization, monomorphization

### 2. Associated Types vs Generic Parameters

**Current**:
```rust
fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult>;
```

**Alternative**:
```rust
trait Analyzer {
    type Output;  // Associated type
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Self::Output;
}
```

### 3. Trait Bounds and Where Clauses

```rust
// Simple bounds
fn register<T: Analyzer + Clone>(analyzer: T) { }

// Complex bounds with where clause  
fn batch_analyze<T>(analyzers: Vec<T>) -> Results
where
    T: Analyzer + Send + Sync + 'static,
    T::Output: Serialize,
{
    // Implementation
}
```

## Better Architectural Patterns

### 1. Async Trait Support

```rust
#[async_trait::async_trait]
pub trait AsyncAnalyzer: Send + Sync {
    async fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult>;
}
```

### 2. Builder Pattern for Configuration

```rust
let analyzer = DuplicateNodesAnalyzer::builder()
    .threshold_meters(10.0)
    .include_ways(true)
    .build()?;
```

### 3. Capability-based System

```rust
#[derive(Debug, Clone)]
pub enum AnalyzerCapability {
    RequiresSpatialIndex,
    SupportsStreaming,
    ThreadSafe,
    Cacheable,
}

trait Analyzer {
    fn capabilities(&self) -> Vec<AnalyzerCapability>;
}
```

## Testing Your Implementation

```bash
# Test the new trait methods
cargo test -p oops-core test_analyzer_trait

# Test the duplicate nodes analyzer
cargo test -p oops-analyzers duplicate_nodes

# Test integration
cargo test --workspace
```

## Common Trait Mistakes

❌ **Object safety violations**: Can't use `Self` in trait objects  
✅ **Design for object safety**: Use `&self` parameters

❌ **Overly complex trait hierarchies**: Many small traits  
✅ **Cohesive interfaces**: Related functionality in one trait

❌ **Ignoring trait bounds**: `Send + Sync` for thread safety  
✅ **Explicit requirements**: Document why bounds are needed

## Key Takeaways

1. **Traits enable polymorphism** - Multiple types, same interface
2. **Default implementations reduce boilerplate** - Common behavior in the trait
3. **Trait objects enable runtime flexibility** - Collection of different analyzers  
4. **Trait bounds express requirements** - Thread safety, serialization, etc.
5. **Registry pattern manages plugins** - Dynamic loading and execution

## Next Steps

With traits mastered, [Lesson 4: Memory Management](rust-lesson-04-memory.md) will explore ownership, borrowing, and efficient data handling in the geospatial context.