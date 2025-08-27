# Implementing Parallel PBF Reading

## Current Implementation

The current PBF reader in `oops-ingest/src/pbf.rs:57` uses sequential processing:

```rust
reader.for_each(|element| {
    // Process each element sequentially
    match element { ... }
})?;
```

This processes OSM elements one by one, accumulating results in mutable state (`osm_data`, `count`, `total_processed`).

## Parallel Implementation Strategy

### 1. Replace Sequential with Parallel Processing

Replace the `for_each` call with `par_map_reduce`:

```rust
let (osm_data, stats) = reader.par_map_reduce(
    |element| {
        // Map phase: process individual elements
        process_element(element, &filter, &self)
    },
    || (OsmData::new(), ProcessingStats::new()),  // Identity value
    |(mut data1, mut stats1), (data2, stats2)| { // Reduce phase
        data1.merge(data2);
        stats1.merge(stats2);
        (data1, stats1)
    }
)?;
```

### 2. Refactor Element Processing

Create a pure function that processes individual elements without mutable state:

```rust
fn process_element(element: Element, filter: &PbfFilter, reader: &PbfReader) -> (OsmData, ProcessingStats) {
    let mut local_data = OsmData::new();
    let mut local_stats = ProcessingStats::new();
    
    local_stats.total_processed = 1;
    
    match element {
        Element::Node(node) => {
            if !filter.include_nodes {
                return (local_data, local_stats);
            }
            
            let osm_node = OsmNode { /* ... */ };
            
            if !reader.node_matches_filter(&osm_node, filter) {
                return (local_data, local_stats);
            }
            
            local_data.add_node(osm_node);
            local_stats.added_count = 1;
        }
        // Similar for Way and Relation...
    }
    
    (local_data, local_stats)
}
```

### 3. Create Mergeable Data Structures

Implement merge functionality for `OsmData`:

```rust
impl OsmData {
    fn merge(&mut self, other: OsmData) {
        // Merge nodes
        self.nodes.extend(other.nodes);
        
        // Merge ways
        self.ways.extend(other.ways);
        
        // Merge relations
        self.relations.extend(other.relations);
    }
}
```

Create a stats structure for tracking progress:

```rust
#[derive(Debug, Default)]
struct ProcessingStats {
    total_processed: usize,
    added_count: usize,
}

impl ProcessingStats {
    fn new() -> Self { Self::default() }
    
    fn merge(&mut self, other: ProcessingStats) {
        self.total_processed += other.total_processed;
        self.added_count += other.added_count;
    }
}
```

### 4. Handle Progress Reporting

Since parallel processing makes individual element progress meaningless, batch report after processing:

```rust
pub async fn read_file_with_progress<P: AsRef<Path>>(
    &self,
    path: P,
    filter: Option<PbfFilter>,
    progress: &dyn ProgressReporter,
) -> Result<OsmData> {
    let filter = filter.unwrap_or_default();
    let reader = ElementReader::from_path(path)?;
    
    // Parallel processing
    let (osm_data, stats) = reader.par_map_reduce(
        |element| process_element(element, &filter, self),
        || (OsmData::new(), ProcessingStats::new()),
        |(mut data1, mut stats1), (data2, stats2)| {
            data1.merge(data2);
            stats1.merge(stats2);
            (data1, stats1)
        }
    )?;
    
    // Single progress report at end
    progress.report_progress(
        stats.total_processed, 
        &format!("Parallel processing complete. Added {} elements", stats.added_count)
    );
    
    Ok(osm_data)
}
```

## Implementation Considerations

### Memory Usage
- Parallel processing may use more memory as each worker maintains partial results
- Consider chunking for very large files to control memory usage

### Thread Safety
- The current filter matching functions are already pure/thread-safe
- No shared mutable state in parallel processing phase

### Error Handling
- `par_map_reduce` returns `Result` so errors propagate correctly
- Individual element processing errors need careful handling

### Performance Tuning
- Default thread count is typically optimal
- Can configure with `RAYON_NUM_THREADS` environment variable
- Consider I/O vs CPU-bound characteristics of your processing

## Streaming API Changes

The `PbfElementStream` would need similar refactoring:

```rust
impl PbfElementStream {
    pub fn par_for_each<F>(self, callback: F) -> Result<()>
    where
        F: Fn(StreamElement) + Send + Sync,
    {
        self.reader.par_map_reduce(
            |element| {
                // Convert and optionally invoke callback
                match element {
                    Element::Node(node) if self.filter.include_nodes => {
                        let osm_node = /* ... */;
                        callback(StreamElement::Node(osm_node));
                    }
                    // etc...
                }
                () // Unit return for par_map_reduce
            },
            || (),
            |(), ()| ()
        )
    }
}
```

## Migration Path

1. **Phase 1**: Implement parallel version alongside existing sequential version
2. **Phase 2**: Add configuration option to choose between sequential/parallel
3. **Phase 3**: Benchmark and tune performance
4. **Phase 4**: Make parallel the default, keep sequential as fallback
5. **Phase 5**: Remove sequential version if no longer needed

## Example Usage

```rust
// Same API, parallel processing under the hood
let reader = PbfReader::new();
let filter = PbfFilter::new().ways_only().with_tag("highway", None);
let data = reader.read_file("input.osm.pbf", Some(filter)).await?;
```

Performance should improve significantly for large files, especially on multi-core systems, with no API changes required.