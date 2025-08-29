# Lesson 4: Memory Management

**Learning Objectives**:
- Master ownership, borrowing, and lifetimes
- Understand when to clone vs borrow vs move
- Optimize memory usage in data-intensive applications
- Handle shared ownership patterns

**Time**: 75 minutes  
**Prerequisites**: Lessons 1-3 completed

## Current Memory Patterns Analysis

### Problem Areas in the Codebase

**1. Unnecessary Cloning** (`oops-core/src/data.rs:148-157`):
```rust
pub fn add_node(&mut self, node: OsmNode) {
    let point = node.to_point();
    let index_entry = NodeIndex {
        node: node.clone(),  // ❌ Unnecessary clone!
        point,
    };
    
    self.node_index.insert(index_entry);
    self.nodes.insert(node.id, node);  // Original moved here
}
```

**Problem**: We clone the entire `OsmNode` just to store it in the spatial index.

**2. Borrowing in Return Types** (`oops-core/src/data.rs:163-184`):
```rust
pub fn nodes_near_point(&self, lat: f64, lon: f64, radius_meters: f64) -> Vec<&OsmNode> {
    // Returns borrowed references - limits caller flexibility
}
```

**Problem**: Returned references are tied to `self`'s lifetime, limiting usage.

**3. String Allocations** (throughout):
```rust
message: Some("Error message".to_string())  // ❌ Allocates every time
```

## Understanding Ownership

### The Three Rules
1. **Each value has one owner**
2. **Only one owner at a time** 
3. **Value is dropped when owner goes out of scope**

### Move Semantics
```rust
let node1 = OsmNode::new(1, 0.0, 0.0);
let node2 = node1;  // node1 is moved, no longer accessible
// println!("{:?}", node1);  // ❌ Compile error!
```

### Borrowing
```rust
let node = OsmNode::new(1, 0.0, 0.0);
let node_ref = &node;  // Immutable borrow
let lat = node_ref.lat;  // Can read through reference
// node is still owned by original binding
```

## Your Tasks

### Task 1: Fix Unnecessary Cloning

Replace the cloning in `add_node` with a more efficient approach:

```rust
// In oops-core/src/data.rs
#[derive(Debug, Clone)]
struct NodeIndex {
    id: OsmId,      // Just store the ID instead of full node
    point: Point<f64>,
}

impl RTreeObject for NodeIndex {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_point([self.point.x(), self.point.y()])
    }
}

impl OsmData {
    pub fn add_node(&mut self, node: OsmNode) {
        let point = node.to_point();
        let index_entry = NodeIndex {
            id: node.id,    // Copy the ID (cheap)
            point,          // Copy the point (cheap)
        };
        
        self.node_index.insert(index_entry);
        self.nodes.insert(node.id, node);  // Move the full node
    }
    
    // Update spatial query methods
    pub fn nodes_near_point(&self, lat: f64, lon: f64, radius_meters: f64) -> Vec<&OsmNode> {
        use geo::HaversineDistance;

        let query_point = Point::new(lon, lat);
        let radius_degrees = radius_meters / 111_000.0;

        let envelope = AABB::from_corners(
            [lon - radius_degrees, lat - radius_degrees],
            [lon + radius_degrees, lat + radius_degrees],
        );

        self.node_index
            .locate_in_envelope(&envelope)
            .filter_map(|node_idx| {
                let distance = query_point.haversine_distance(&node_idx.point);
                if distance <= radius_meters {
                    self.nodes.get(&node_idx.id)  // Look up by ID
                } else {
                    None
                }
            })
            .collect()
    }
    
    pub fn nodes_in_bbox(&self, bbox: &BoundingBox) -> Vec<&OsmNode> {
        let envelope = AABB::from_corners([bbox.min_lon, bbox.min_lat], [bbox.max_lon, bbox.max_lat]);

        self.node_index
            .locate_in_envelope(&envelope)
            .filter_map(|node_idx| {
                if bbox.contains_point(node_idx.point.x(), node_idx.point.y()) {
                    self.nodes.get(&node_idx.id)  // Look up by ID
                } else {
                    None
                }
            })
            .collect()
    }
}
```

### Task 2: Optimize String Handling

Create a string interning system to reduce allocations:

```rust
// In oops-core/src/data.rs
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// String interning for common OSM tags and values
#[derive(Debug, Default)]
pub struct StringInterner {
    strings: HashMap<String, Arc<str>>,
}

impl StringInterner {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn intern(&mut self, s: &str) -> Arc<str> {
        if let Some(existing) = self.strings.get(s) {
            existing.clone()
        } else {
            let arc_str: Arc<str> = Arc::from(s);
            self.strings.insert(s.to_string(), arc_str.clone());
            arc_str
        }
    }
}

// Usage in OsmData
impl OsmData {
    pub fn new_with_interner() -> Self {
        Self {
            nodes: HashMap::new(),
            ways: HashMap::new(),
            relations: HashMap::new(),
            node_index: RTree::new(),
            // Could add string interner for tags
        }
    }
}
```

### Task 3: Implement Cow (Clone-on-Write) for Tags

Handle tag storage more efficiently:

```rust
use std::borrow::Cow;
use std::collections::HashMap;

// Alternative tag storage that can borrow or own strings
pub type CowTags<'a> = HashMap<Cow<'a, str>, Cow<'a, str>>;

#[derive(Debug, Clone)]
pub struct OsmNodeCow<'a> {
    pub id: OsmId,
    pub lat: f64,
    pub lon: f64,
    pub tags: CowTags<'a>,
}

impl<'a> OsmNodeCow<'a> {
    pub fn new_borrowed(id: OsmId, lon: f64, lat: f64) -> Self {
        Self {
            id,
            lat,
            lon,
            tags: HashMap::new(),
        }
    }
    
    pub fn add_tag(&mut self, key: &'a str, value: &'a str) {
        self.tags.insert(Cow::Borrowed(key), Cow::Borrowed(value));
    }
    
    pub fn add_tag_owned(&mut self, key: String, value: String) {
        self.tags.insert(Cow::Owned(key), Cow::Owned(value));
    }
    
    pub fn into_owned(self) -> OsmNode {
        let owned_tags = self.tags
            .into_iter()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
            
        OsmNode {
            id: self.id,
            lat: self.lat,
            lon: self.lon,
            tags: owned_tags,
        }
    }
}
```

### Task 4: Smart Pointer Usage

Implement shared ownership for large datasets:

```rust
use std::sync::Arc;
use std::rc::Rc;

// For single-threaded scenarios - cheaper than Arc
pub type RcOsmData = Rc<OsmData>;

// For multi-threaded scenarios
pub type ArcOsmData = Arc<OsmData>;

// Builder that can share data efficiently
pub struct AnalysisBuilder {
    data: ArcOsmData,
    analyzers: Vec<Box<dyn Analyzer>>,
}

impl AnalysisBuilder {
    pub fn new(data: OsmData) -> Self {
        Self {
            data: Arc::new(data),
            analyzers: Vec::new(),
        }
    }
    
    pub fn add_analyzer(mut self, analyzer: Box<dyn Analyzer>) -> Self {
        self.analyzers.push(analyzer);
        self
    }
    
    pub fn run_parallel(self, query: AnalysisQuery) -> Vec<Vec<AnalysisResult>> {
        use std::thread;
        
        let handles: Vec<_> = self.analyzers
            .into_iter()
            .map(|analyzer| {
                let data = Arc::clone(&self.data);  // Cheap clone of Arc
                let query = query.clone();
                
                thread::spawn(move || {
                    analyzer.analyze(&query, &data)
                })
            })
            .collect();
        
        handles.into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    }
}
```

### Task 5: Lifetime Management

Create a borrowed data structure for temporary analysis:

```rust
// Temporary analysis view that borrows from main data
#[derive(Debug)]
pub struct OsmDataView<'a> {
    nodes: &'a HashMap<OsmId, OsmNode>,
    ways: &'a HashMap<OsmId, OsmWay>,
    spatial_filter: Option<BoundingBox>,
}

impl<'a> OsmDataView<'a> {
    pub fn new(data: &'a OsmData) -> Self {
        Self {
            nodes: &data.nodes,
            ways: &data.ways,
            spatial_filter: None,
        }
    }
    
    pub fn with_bbox_filter(mut self, bbox: BoundingBox) -> Self {
        self.spatial_filter = Some(bbox);
        self
    }
    
    pub fn filtered_nodes(&self) -> impl Iterator<Item = &'a OsmNode> {
        self.nodes.values().filter(move |node| {
            if let Some(bbox) = &self.spatial_filter {
                bbox.contains_point(node.lon, node.lat)
            } else {
                true
            }
        })
    }
    
    pub fn ways_using_node(&self, node_id: OsmId) -> impl Iterator<Item = &'a OsmWay> {
        self.ways.values().filter(move |way| way.nodes.contains(&node_id))
    }
}

// Usage example
fn analyze_intersection<'a>(
    intersection_id: OsmId,
    data_view: &'a OsmDataView<'a>
) -> Option<IntersectionAnalysis<'a>> {
    let ways: Vec<_> = data_view.ways_using_node(intersection_id).collect();
    
    if ways.len() >= 3 {
        Some(IntersectionAnalysis {
            node_id: intersection_id,
            connected_ways: ways,
        })
    } else {
        None
    }
}

#[derive(Debug)]
struct IntersectionAnalysis<'a> {
    node_id: OsmId,
    connected_ways: Vec<&'a OsmWay>,
}
```

## Memory Optimization Patterns

### 1. Arena Allocation

For many small allocations of similar types:

```rust
use typed_arena::Arena;

pub struct NodeArena {
    nodes: Arena<OsmNode>,
}

impl NodeArena {
    pub fn new() -> Self {
        Self {
            nodes: Arena::new(),
        }
    }
    
    pub fn add_node(&self, id: OsmId, lon: f64, lat: f64) -> &OsmNode {
        self.nodes.alloc(OsmNode::new(id, lon, lat))
    }
}
```

### 2. Custom Allocators

For specific memory patterns:

```rust
use bumpalo::Bump;

pub struct BumpOsmData {
    bump: Bump,
    nodes: HashMap<OsmId, &OsmNode>,
}

impl BumpOsmData {
    pub fn new() -> Self {
        Self {
            bump: Bump::new(),
            nodes: HashMap::new(),
        }
    }
    
    pub fn add_node(&mut self, node: OsmNode) -> &OsmNode {
        let node_ref = self.bump.alloc(node);
        self.nodes.insert(node_ref.id, node_ref);
        node_ref
    }
}
```

### 3. Zero-Copy Deserialization

For reading data formats:

```rust
use serde::{Deserialize};
use zerocopy::{AsBytes, FromBytes};

#[derive(Debug, AsBytes, FromBytes)]
#[repr(C)]
struct CompactNode {
    id: u64,
    lat: f32,  // Reduced precision for space
    lon: f32,
}

impl From<&[u8]> for CompactNode {
    fn from(bytes: &[u8]) -> Self {
        *CompactNode::from_bytes(&bytes[..12]).unwrap()
    }
}
```

## Common Memory Mistakes

❌ **Clone when move suffices**: `let x = y.clone()` vs `let x = y`  
✅ **Move by default, clone when needed**

❌ **Borrowing everything**: Lifetime complexity  
✅ **Own when appropriate, borrow for temporary access**

❌ **Ignoring drop semantics**: Memory leaks in unsafe code  
✅ **Leverage RAII and destructors**

❌ **Boxing unnecessarily**: `Box<i32>` instead of `i32`  
✅ **Stack allocation for small, known-size data**

## Performance Testing

Create benchmarks to measure improvements:

```rust
// In oops-core/benches/memory.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oops_core::*;

fn bench_node_insertion(c: &mut Criterion) {
    c.bench_function("add_node_with_clone", |b| {
        b.iter(|| {
            let mut data = OsmData::new();
            for i in 0..1000 {
                let node = OsmNode::new(i, 0.0, 0.0);
                data.add_node(black_box(node));  // Old version with clone
            }
        })
    });
    
    c.bench_function("add_node_optimized", |b| {
        b.iter(|| {
            let mut data = OsmData::new();
            for i in 0..1000 {
                let node = OsmNode::new(i, 0.0, 0.0);
                data.add_node(black_box(node));  // New version without clone
            }
        })
    });
}

criterion_group!(benches, bench_node_insertion);
criterion_main!(benches);
```

Run with: `cargo bench`

## Memory Profiling

Use tools to measure actual memory usage:

```bash
# Install valgrind (Linux/macOS)
cargo install cargo-valgrind

# Run with memory checking
cargo valgrind test

# Or use heaptrack
cargo install cargo-profiler
cargo profiler callgrind --bin oops-cli

# Simple memory usage tracking
cargo run --bin oops-cli -- analyze --file sample_data/slc_small.osm.pbf --format summary
```

## Key Takeaways

1. **Ownership eliminates memory safety bugs** - No dangling pointers or double frees
2. **Move semantics are zero-cost** - Transfers ownership without copying
3. **Borrowing enables flexible APIs** - Access without ownership transfer
4. **Smart pointers handle shared ownership** - Rc for single-thread, Arc for multi-thread
5. **Clone is explicit and visible** - Makes allocation costs obvious
6. **Lifetimes prevent use-after-free** - Compile-time memory safety guarantees

## Next Steps

With memory management mastered, [Lesson 5: Error Handling Patterns](rust-lesson-05-errors.md) will cover robust error handling strategies for the geospatial domain.

## Advanced Topics

For further exploration:
- **Custom allocators** for specialized workloads
- **Memory mapping** for large files  
- **SIMD optimization** for geometric calculations
- **Lock-free data structures** for concurrent access