# Pedestrian Quality Analyzer - Rust Implementation Plan

## Overview
Implement the pedestrian mapping quality analyzer as a native Rust analyzer within the existing OOPS architecture. This analyzer assesses intersection quality using a 6-level system from Critical (no infrastructure) to Accessibility (full compliance).

## Current Architecture Assessment

### ✅ What We Have
- **Core Data Structures**: `OsmNode`, `OsmWay`, `OsmData` with spatial indexing
- **Analyzer Framework**: `Analyzer` trait, `AnalysisResult`, `AnalyzerRegistry`
- **Spatial Foundation**: R-tree indexing, `geo` crate integration
- **CLI Interface**: Working command system with multiple output formats

### 🔧 What Needs Extension
- **Clustering Algorithms**: DBSCAN for grouping nearby intersections
- **Network Analysis**: Graph operations on road networks
- **Quality Assessment**: 6-level evaluation system
- **Complex Results**: Rich metadata and geographic features
- **Coordinate Transforms**: CRS management for accurate distance calculations

---

## Phase 8: Advanced Spatial Operations

### Step 23: Clustering Infrastructure
**Timeline**: 1-2 days
**Dependencies**: Current `geo` and `rstar` setup

#### Implementation Tasks:
- [ ] **Add DBSCAN Implementation**: 
  ```rust
  pub struct DbscanClusterer {
      eps: f64,           // Distance threshold
      min_samples: usize, // Minimum cluster size
  }
  ```
- [ ] **Spatial Distance Calculations**: Projected coordinate distance functions
- [ ] **Cluster Result Types**:
  ```rust
  pub struct Cluster {
      pub id: ClusterId,
      pub centroid: Point,
      pub members: Vec<NodeId>,
      pub representative_node: NodeId,
  }
  ```

#### Testing Strategy:
- Unit tests with known point sets
- Validate against Python DBSCAN results
- Test edge cases (single points, dense clusters)

### Step 24: Network Analysis Extensions
**Timeline**: 1-2 days
**Dependencies**: Step 23

#### Implementation Tasks:
- [ ] **Intersection Detection**: 
  ```rust
  pub fn find_road_intersections(
      graph: &NetworkGraph, 
      config: &IntersectionConfig
  ) -> Vec<NodeId>
  ```
- [ ] **Highway Type Filtering**: Configurable road type inclusion/exclusion
- [ ] **Graph Building**: Convert `OsmData` to network graph structure
- [ ] **Degree Analysis**: Identify nodes with 3+ connections

#### Configuration:
```rust
pub struct IntersectionConfig {
    pub excluded_highway_types: HashSet<String>,
    pub service_highway_types: HashSet<String>,
    pub major_highway_types: HashSet<String>,
    pub major_roads_only: bool,
    pub exclude_service_roads: bool,
}
```

### Step 25: Coordinate Reference System (CRS) Management
**Timeline**: 1 day
**Dependencies**: None (can be done in parallel)

#### Implementation Tasks:
- [ ] **CRS Detection**: Automatic optimal projected CRS selection
- [ ] **Transformation Pipeline**: 
  ```rust
  pub struct CrsManager {
      pub fn get_optimal_projected_crs(bbox: BoundingBox) -> String;
      pub fn transform_point(point: Point, from: &str, to: &str) -> Point;
  }
  ```
- [ ] **Integration with Clustering**: Ensure distance calculations use projected coordinates

#### Dependencies:
- Consider `proj` crate for transformations
- Fallback to Web Mercator (EPSG:3857) if proj unavailable

---

## Phase 9: Quality Assessment System

### Step 26: Quality Level Framework
**Timeline**: 1-2 days
**Dependencies**: Step 23-25

#### Implementation Tasks:
- [ ] **Quality Level Enum**:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
  pub enum PedestrianQualityLevel {
      Critical = 0,    // No infrastructure
      Minimal = 1,     // Tagged only  
      Basic = 2,       // Mapped sidewalks
      Connected = 3,   // Has crossings
      Safety = 4,      // Complete infrastructure  
      Accessibility = 5 // Full details
  }
  ```

- [ ] **Quality Requirements System**:
  ```rust
  pub struct QualityRequirements {
      pub sidewalk_requirements: Vec<SidewalkRequirement>,
      pub crossing_requirements: Vec<CrossingRequirement>,
      pub curb_requirements: Vec<CurbRequirement>,
      // ...
  }
  ```

- [ ] **Assessment Engine**:
  ```rust
  pub struct QualityLevelAssessor {
      pub fn assess_intersection(
          &self,
          cluster_nodes: &[NodeId],
          nodes_gdf: &OsmData,
          edges_gdf: &OsmData
      ) -> QualityResult;
  }
  ```

### Step 27: Quality Result Types
**Timeline**: 1 day
**Dependencies**: Step 26

#### Implementation Tasks:
- [ ] **Rich Result Structure**:
  ```rust
  pub struct QualityResult {
      pub quality_level: PedestrianQualityLevel,
      pub quality_score: f64,
      pub cluster_id: ClusterId,
      pub centroid_lat: f64,
      pub centroid_lon: f64,
      pub representative_osm_id: OsmId,
      pub missing_infrastructure: MissingInfrastructure,
      pub actionable_recommendations: Vec<String>,
      pub next_level_requirements: Vec<String>,
      pub analysis_details: HashMap<String, String>,
      // Counts
      pub node_count: usize,
      pub crossing_count: usize,
      pub curb_count: usize,
  }
  ```

- [ ] **Missing Infrastructure Tracking**:
  ```rust
  pub struct MissingInfrastructure {
      pub ways: HashMap<String, Vec<String>>,  // Missing way features
      pub nodes: HashMap<String, Vec<String>>, // Missing node features  
  }
  ```

---

## Phase 10: Pedestrian Analyzer Implementation

### Step 28: Core Analyzer Structure
**Timeline**: 2-3 days
**Dependencies**: Step 26-27

#### Implementation Tasks:
- [ ] **Main Analyzer**:
  ```rust
  pub struct PedestrianAnalyzer {
      config: PedestrianConfig,
      crs_manager: CrsManager,
      clusterer: DbscanClusterer,
      quality_assessor: QualityLevelAssessor,
  }
  ```

- [ ] **Configuration System**:
  ```rust
  pub struct PedestrianConfig {
      pub pedestrian_tags_node: HashSet<String>,
      pub pedestrian_tags_way: HashSet<String>,
      pub cluster_distance: f64,
      pub min_samples_for_cluster: usize,
      pub auto_detect_crs: bool,
      pub projected_crs: Option<String>,
      // Highway filtering
      pub excluded_highway_types: HashSet<String>,
      pub major_highway_types: HashSet<String>,
      pub major_roads_only: bool,
  }
  ```

- [ ] **Analysis Pipeline**:
  1. Extract street network with pedestrian tags
  2. Find road intersections (degree > 2)
  3. Cluster nearby intersections using DBSCAN
  4. Assess quality for each cluster
  5. Generate rich results with recommendations

### Step 29: Integration with Existing Architecture
**Timeline**: 1 day
**Dependencies**: Step 28

#### Implementation Tasks:
- [ ] **Extend AnalysisResult**: Support geographic features and complex metadata
  ```rust
  pub struct AnalysisResult {
      pub element_id: OsmId,
      pub element_type: ElementType,
      pub findings: Vec<Finding>,
      pub overall_severity: Severity,
      pub confidence: f64,
      // NEW: Support for geographic results
      pub geometry: Option<geo::Geometry>,
      pub rich_metadata: HashMap<String, serde_json::Value>,
  }
  ```

- [ ] **Implement Analyzer Trait**: Convert `QualityResult` to `AnalysisResult`
- [ ] **Register with System**: Add to `AnalyzerRegistry`
- [ ] **CLI Integration**: Ensure output formats handle rich results

---

## Phase 11: Testing & Validation

### Step 30: Comprehensive Testing
**Timeline**: 2 days
**Dependencies**: Step 28-29

#### Testing Strategy:
- [ ] **Unit Tests**: Each component (clustering, quality assessment, CRS)
- [ ] **Integration Tests**: Full analyzer with Salt Lake City data
- [ ] **Reference Validation**: Compare results with Python implementation
- [ ] **Performance Tests**: Memory usage and processing time with large datasets

#### Test Data:
- Reuse existing Salt Lake City OSM sample
- Create minimal synthetic test cases for specific scenarios
- Test all 6 quality levels with known intersections

### Step 31: Documentation & Examples
**Timeline**: 1 day
**Dependencies**: Step 30

#### Documentation Tasks:
- [ ] **API Documentation**: Comprehensive rustdoc comments
- [ ] **Configuration Guide**: All parameters and their effects
- [ ] **Quality Level Guide**: Detailed explanation of 6-level system
- [ ] **Example Usage**: CLI commands and expected output

---

## Dependencies & Crates

### New Dependencies Needed:
```toml
# Clustering
kdtree = "0.6"  # For efficient nearest neighbor searches (DBSCAN)

# Coordinate transformations (optional)
proj = "0.27"   # CRS transformations

# Enhanced collections
indexmap = "2.0"  # Ordered maps for consistent results
```

### Existing Dependencies (Already Available):
- `geo` - Geospatial operations
- `rstar` - Spatial indexing  
- `serde` - Serialization
- Standard collections and algorithms

---

## Implementation Phases Timeline

| Phase | Description | Timeline | Dependencies |
|-------|-------------|----------|--------------|
| 8     | Spatial Operations | 3-4 days | Current architecture |
| 9     | Quality Framework | 2-3 days | Phase 8 |
| 10    | Main Analyzer | 3-4 days | Phase 8, 9 |
| 11    | Testing & Docs | 2-3 days | Phase 10 |

**Total Estimated Timeline**: 10-14 days

---

## Key Design Decisions

### 1. Native Rust vs Python Integration
✅ **Choice**: Pure Rust implementation
- **Pros**: No external dependencies, type safety, performance, single deployment
- **Cons**: More initial development time
- **Rationale**: Better long-term maintainability and fits existing architecture

### 2. Clustering Algorithm
✅ **Choice**: Custom DBSCAN implementation using `kdtree`
- **Alternative**: Could use external ML crate, but DBSCAN is simple enough
- **Rationale**: Minimal dependencies, full control over algorithm

### 3. Quality Assessment
✅ **Choice**: Rule-based system with configurable requirements
- **Rationale**: Transparent, debuggable, matches existing analyzer patterns

### 4. Result Format
✅ **Choice**: Extend existing `AnalysisResult` rather than create new types
- **Rationale**: Maintains compatibility with CLI and output systems

---

## Risk Mitigation

### Technical Risks:
1. **Coordinate Transformations**: Fallback to Web Mercator if `proj` unavailable
2. **Clustering Performance**: Use spatial indexing to limit search space
3. **Memory Usage**: Stream processing for large datasets

### Integration Risks:
1. **Result Format Changes**: Maintain backward compatibility with existing analyzers
2. **Configuration Complexity**: Provide sensible defaults, comprehensive validation
3. **Testing Coverage**: Extensive unit tests for each component

---

## Success Criteria

### Functional Requirements:
- [ ] Produces same quality assessments as Python reference implementation
- [ ] Handles intersection clustering correctly
- [ ] Provides actionable recommendations for each quality level
- [ ] Integrates seamlessly with existing CLI and output formats

### Performance Requirements:
- [ ] Processes Salt Lake City dataset in <30 seconds
- [ ] Memory usage proportional to result count, not input size
- [ ] Scales to city-sized datasets (100k+ intersections)

### Quality Requirements:
- [ ] >90% test coverage for core algorithms
- [ ] Comprehensive documentation for configuration options
- [ ] Clear examples for each quality level

This implementation plan transforms the Python analyzer concept into a native Rust analyzer that fits seamlessly into the existing OOPS architecture while providing the sophisticated pedestrian infrastructure analysis capabilities demonstrated in the Python example.