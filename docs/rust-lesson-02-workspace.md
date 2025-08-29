# Lesson 2: Workspace Architecture

**Learning Objectives**:
- Understand Cargo workspace organization
- Learn crate boundaries and dependencies
- Practice navigating multi-crate projects

**Time**: 45 minutes  
**Prerequisites**: Lesson 1 completed

## Project Structure Analysis

The OOPS project uses a **workspace architecture**:

```
oops/
├── Cargo.toml          # Workspace root
├── oops-core/          # Core data structures & traits
├── oops-ingest/        # Data loading (PBF files, API)
├── oops-analyzers/     # Quality analysis implementations
└── oops-cli/          # Command-line interface
```

## Understanding Workspaces

### Root Cargo.toml
```toml
[workspace]
members = [
    "oops-core",
    "oops-ingest", 
    "oops-analyzers",
    "oops-cli"
]
resolver = "2"

[workspace.dependencies]
geo = "0.28"
serde = { version = "1.0", features = ["derive"] }
# ... shared dependencies
```

**Key Concepts**:
- **Workspace**: Collection of related crates
- **Members**: Individual crates in the workspace
- **Shared dependencies**: Version coordination across crates

## Crate Responsibilities

### 1. oops-core (Foundation)
**Purpose**: Core data structures and plugin system
**Key files**:
- `data.rs` - OSM data structures (Node, Way, Relation)
- `analysis.rs` - Analysis framework (Analyzer trait, Results)

**Design principle**: No external dependencies beyond geo/serde

### 2. oops-ingest (Data Layer)
**Purpose**: Loading data from external sources
**Key responsibilities**:
- PBF file parsing
- Overpass API queries
- Data filtering and transformation

### 3. oops-analyzers (Business Logic)  
**Purpose**: Concrete analysis implementations
**Current analyzers**:
- `MissingMaxspeedAnalyzer`
- `MissingStreetNamesAnalyzer`

### 4. oops-cli (Interface Layer)
**Purpose**: User-facing command-line interface
**Key features**:
- Argument parsing with clap
- Output formatting (JSON, CSV, summary)
- Analyzer orchestration

## Your Tasks

### Task 1: Explore Dependencies

Run these commands and analyze the output:

```bash
# Show dependency tree
cargo tree

# Check for duplicate dependencies
cargo tree --duplicates

# Show workspace info
cargo metadata --format-version 1 | jq '.workspace_members'
```

**Questions to answer**:
1. Which crate depends on `tokio`? Why?
2. How many versions of `serde` are in use?
3. What external crates does `oops-core` depend on?

### Task 2: Analyze Crate Boundaries

Look at these import statements:
```rust
// In oops-cli/src/main.rs
use oops_analyzers::{MissingMaxspeedAnalyzer, MissingStreetNamesAnalyzer};
use oops_core::{AnalysisQuery, AnalysisResult, AnalyzerRegistry, BoundingBox};
use oops_ingest::{PbfFilter, PbfReader};
```

**Design Questions**:
1. Why doesn't `oops-cli` import from `oops-core` directly?
2. What would break if `oops-analyzers` depended on `oops-cli`?
3. How does this structure support plugin development?

### Task 3: Add a New Utility Function

Add this function to `oops-core/src/data.rs`:

```rust
impl OsmData {
    /// Count nodes by type of amenity
    pub fn count_amenities(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();
        
        for node in self.nodes.values() {
            if let Some(amenity) = node.tags.get("amenity") {
                *counts.entry(amenity.clone()).or_insert(0) += 1;
            }
        }
        
        counts
    }
}
```

Then test it's available in other crates:
```rust
// In oops-cli/src/main.rs (add to the analyze_command function)
if verbose {
    let amenity_counts = data.count_amenities();
    println!("Found amenities: {:?}", amenity_counts);
}
```

**Learning points**:
- Public APIs flow from core → dependents
- Changes in core affect all dependent crates
- `pub` keyword controls visibility across crate boundaries

## Architecture Patterns

### 1. Layered Architecture
```
CLI Layer (oops-cli)
    ↓
Business Logic (oops-analyzers)
    ↓
Core Domain (oops-core)
    ↓
Data Access (oops-ingest)
```

### 2. Plugin Pattern
```rust
// Core defines the interface
pub trait Analyzer {
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult>;
}

// Implementations live in separate crate
pub struct MissingMaxspeedAnalyzer;
impl Analyzer for MissingMaxspeedAnalyzer { /* ... */ }
```

### 3. Registry Pattern
```rust
// Runtime plugin registration
let mut registry = AnalyzerRegistry::new();
registry.register(MissingMaxspeedAnalyzer::new());
registry.register(CustomAnalyzer::new());
```

## Better Patterns to Consider

### Current Issues
1. **Circular dependencies**: Some imports suggest tight coupling
2. **Feature flags**: All dependencies are always compiled
3. **Versioning**: No semantic versioning between workspace crates

### Improvements
```rust
// 1. Feature flags for optional analyzers
[features]
default = ["basic-analyzers"]
basic-analyzers = ["missing-maxspeed", "missing-names"]
advanced-analyzers = ["pedestrian-crossing", "accessibility"]

// 2. Explicit versioning
[dependencies]
oops-core = { version = "0.1.0", path = "../oops-core" }

// 3. API stability markers
#[deprecated = "Use analyze_with_config instead"]
pub fn analyze(&self) -> Results { }
```

## Common Workspace Commands

```bash
# Build all crates
cargo build --workspace

# Test specific crate
cargo test -p oops-core

# Run CLI with workspace context
cargo run -p oops-cli -- --help

# Check all crates
cargo check --workspace

# Clean everything
cargo clean
```

## Design Exercise

**Scenario**: You want to add a new analyzer for bicycle infrastructure.

**Questions**:
1. Which crate should contain `BicycleAnalyzer`?
2. What dependencies would it need?
3. How would it integrate with the existing system?
4. What data structures might need extending?

**Implementation outline**:
```rust
// In oops-analyzers/src/bicycle.rs
pub struct BicycleAnalyzer {
    config: AnalyzerConfig,
}

impl Analyzer for BicycleAnalyzer {
    fn name(&self) -> &str { "bicycle_infrastructure" }
    
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        // Find ways tagged with bicycle=*
        // Check for missing bicycle infrastructure
        // Return findings
    }
}
```

## Key Takeaways

1. **Workspaces organize related functionality** - Better than monolithic crates
2. **Dependency direction matters** - Core shouldn't depend on applications  
3. **Clear boundaries enable modularity** - Easy to add new analyzers
4. **Shared dependencies reduce duplication** - Workspace.dependencies is powerful
5. **Public APIs need careful design** - They're contracts between crates

## Next Steps

Understanding the workspace prepares you for [Lesson 3: Trait System & Plugins](rust-lesson-03-traits.md), where we'll dive deep into how the analyzer system works.

## Troubleshooting

**Problem**: `cargo build` fails with dependency errors  
**Solution**: Check if all workspace members are properly listed

**Problem**: Can't import types from other crates  
**Solution**: Verify the crate is listed in `Cargo.toml` dependencies

**Problem**: Changes don't take effect  
**Solution**: Try `cargo clean` to rebuild everything