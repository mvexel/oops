# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

OOPS (OSM Object Problem Scanner) is a modular Rust toolkit for analyzing OpenStreetMap data quality issues. It uses a workspace structure with four main crates:

- **oops-core**: Core data structures (`OsmNode`, `OsmWay`, `OsmRelation`, `OsmData`) and analysis framework
- **oops-ingest**: Data loading from Overpass API and PBF files  
- **oops-analyzers**: Quality analysis implementations using composable architecture
- **oops-cli**: Command-line interface with multiple output formats

## Common Development Commands

### Build and Run
```bash
# Build entire workspace
cargo build --release

# Run CLI with different commands
cargo run --bin oops-cli -- list
cargo run --bin oops-cli -- analyze --file data.osm.pbf --verbose
cargo run --bin oops-cli -- analyze --file data.osm.pbf --format summary
```

### Testing
```bash
# Unit tests
cargo test

# Integration tests with sample data (requires large files)
cargo test -- --ignored

# Specific test with output
cargo test test_pbf_reader_slc_small -- --ignored --nocapture

# Run specific crate tests
cd oops-core && cargo test
```

### Benchmarks
```bash
# Run spatial performance benchmarks
cd oops-core && cargo bench
```

## Architecture Details

### Core Data Model
- Uses `geo` crate for geometric operations and spatial types
- Spatial indexing with `rstar` for efficient queries
- Memory-efficient streaming with custom parsers
- Plugin architecture with analyzer registry system

### Analysis Framework
The analysis system uses a composable architecture:
- **Selectors**: Choose which OSM elements to analyze
- **Context Providers**: Gather spatial or relational context
- **Detectors**: Identify specific quality issues
- **Analyzers**: Combine components for complete analysis

### Spatial Operations
- R-tree spatial indexing for fast neighbor queries
- SIMD-optimized geometric calculations where applicable
- Intersection detection algorithms for complex geometries
- Bounding box queries and spatial filtering

## Key Dependencies
- `geo`: Core geospatial operations and types
- `rstar`: Spatial indexing (R-tree implementation)
- `osmpbf`: Protocol Buffer format parsing
- `reqwest`: HTTP client for Overpass API
- `petgraph`: Graph algorithms for network analysis
- `rayon`: Parallel processing
- `criterion`: Benchmarking framework

## Development Guidelines
- Adhere to test driven development practices
- Make use of well maintained geospatial crates https://github.com/georust
- Use the existing plugin system for new analyzers
- Test with real OSM data when possible
- Follow existing patterns for spatial operations and memory management