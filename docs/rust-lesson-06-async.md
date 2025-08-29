# Lesson 6: Async Programming

**Learning Objectives**:
- Understand async/await and futures in Rust
- Convert synchronous analyzers to async
- Implement parallel processing for large datasets
- Handle async error propagation and cancellation

**Time**: 60 minutes  
**Prerequisites**: Lessons 1-5 completed

## Current Synchronous Limitations

### Problem 1: Blocking I/O
```rust
// In oops-ingest - blocks entire thread
let data = reader.read_file(&pbf_path, filter).await?;

// Analysis runs sequentially
for analyzer in &analyzers {
    let results = analyzer.analyze(&query, &data);  // Blocking
}
```

### Problem 2: No Concurrent Analysis
```rust
// All analyzers run in sequence
let results = if let Some(analyzer_name) = analyzer {
    vec![registry.analyze(&analyzer_name, &query, &data)?]
} else {
    registry.analyze_all(&query, &data).into_values().collect()
};
```

### Problem 3: Large Dataset Memory Usage
- Entire PBF file loaded into memory
- No streaming analysis capability  
- Cannot process datasets larger than RAM

## Understanding Async Rust

### Futures and Async Functions
```rust
use std::future::Future;

// Async function returns impl Future
async fn fetch_data() -> Result<String, reqwest::Error> {
    let response = reqwest::get("https://api.example.com/data").await?;
    response.text().await
}

// Manual Future implementation
struct DelayedResult<T> {
    value: Option<T>,
    delay: std::time::Duration,
}

impl<T> Future for DelayedResult<T> {
    type Output = T;
    
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        // Implementation details...
        std::task::Poll::Pending
    }
}
```

### Tokio Runtime
```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tokio provides the async runtime
    let result = async_operation().await?;
    Ok(())
}
```

## Your Tasks

### Task 1: Create Async Analyzer Trait

Create a new async trait in `oops-core/src/analysis.rs`:

```rust
use async_trait::async_trait;
use std::future::Future;
use tokio::task::JoinHandle;

/// Async version of the Analyzer trait for non-blocking analysis
#[async_trait]
pub trait AsyncAnalyzer: Send + Sync {
    /// Analyzer identification
    fn name(&self) -> &str;
    fn version(&self) -> &str { "1.0.0" }
    fn description(&self) -> &str;
    
    /// Async analysis method
    async fn analyze_async(
        &self,
        query: &AnalysisQuery,
        data: &OsmData,
    ) -> Result<Vec<AnalysisResult>, AnalysisError>;
    
    /// Estimate processing time (for progress reporting)
    fn estimated_duration(&self, data: &OsmData) -> std::time::Duration {
        std::time::Duration::from_millis(data.nodes.len() as u64 + data.ways.len() as u64)
    }
    
    /// Check if this analyzer can run concurrently with others
    fn supports_concurrent_execution(&self) -> bool {
        true  // Most analyzers are read-only
    }
}

/// Wrapper to make sync analyzers work with async interface
pub struct AsyncAnalyzerWrapper<T: Analyzer> {
    inner: T,
}

impl<T: Analyzer> AsyncAnalyzerWrapper<T> {
    pub fn new(analyzer: T) -> Self {
        Self { inner: analyzer }
    }
}

#[async_trait]
impl<T: Analyzer + 'static> AsyncAnalyzer for AsyncAnalyzerWrapper<T> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    
    fn version(&self) -> &str {
        self.inner.version()
    }
    
    fn description(&self) -> &str {
        self.inner.description()
    }
    
    async fn analyze_async(
        &self,
        query: &AnalysisQuery,
        data: &OsmData,
    ) -> Result<Vec<AnalysisResult>, AnalysisError> {
        // Run sync analyzer on thread pool to avoid blocking
        let query = query.clone();
        let data = data.clone();  // TODO: Optimize this clone
        
        tokio::task::spawn_blocking(move || {
            self.inner.analyze(&query, &data)
        }).await
        .map_err(|e| AnalysisError::AnalysisFailed {
            element_id: 0,
            reason: format!("Task join error: {}", e),
        })?
    }
}
```

Add to `oops-core/Cargo.toml`:
```toml
[dependencies]
async-trait = "0.1"
tokio = { version = "1.0", features = ["rt-multi-thread", "macros", "time"] }
```

### Task 2: Async Registry with Parallel Execution

Extend the registry to support async analyzers:

```rust
use std::collections::HashMap;
use tokio::task::JoinHandle;
use futures::future::join_all;

/// Registry that supports both sync and async analyzers
pub struct AsyncAnalyzerRegistry {
    sync_analyzers: HashMap<String, Box<dyn Analyzer>>,
    async_analyzers: HashMap<String, Box<dyn AsyncAnalyzer>>,
}

impl AsyncAnalyzerRegistry {
    pub fn new() -> Self {
        Self {
            sync_analyzers: HashMap::new(),
            async_analyzers: HashMap::new(),
        }
    }
    
    /// Register a sync analyzer (wrapped for async compatibility)
    pub fn register_sync<A: Analyzer + 'static>(&mut self, analyzer: A) {
        let name = analyzer.name().to_string();
        let wrapper = AsyncAnalyzerWrapper::new(analyzer);
        self.async_analyzers.insert(name, Box::new(wrapper));
    }
    
    /// Register a native async analyzer
    pub fn register_async<A: AsyncAnalyzer + 'static>(&mut self, analyzer: A) {
        let name = analyzer.name().to_string();
        self.async_analyzers.insert(name, Box::new(analyzer));
    }
    
    /// Run all analyzers concurrently
    pub async fn analyze_all_concurrent(
        &self,
        query: &AnalysisQuery,
        data: &OsmData,
    ) -> HashMap<String, Result<Vec<AnalysisResult>, AnalysisError>> {
        let mut handles = Vec::new();
        
        for (name, analyzer) in &self.async_analyzers {
            let analyzer_name = name.clone();
            let query = query.clone();
            let data = data.clone();  // TODO: Use Arc to avoid clones
            
            let handle = tokio::spawn(async move {
                let result = analyzer.analyze_async(&query, &data).await;
                (analyzer_name, result)
            });
            
            handles.push(handle);
        }
        
        // Wait for all analyzers to complete
        let results = join_all(handles).await;
        
        let mut final_results = HashMap::new();
        for join_result in results {
            match join_result {
                Ok((name, analysis_result)) => {
                    final_results.insert(name, analysis_result);
                }
                Err(join_error) => {
                    eprintln!("Task join failed: {}", join_error);
                }
            }
        }
        
        final_results
    }
    
    /// Run specific analyzer with timeout
    pub async fn analyze_with_timeout(
        &self,
        analyzer_name: &str,
        query: &AnalysisQuery,
        data: &OsmData,
        timeout: std::time::Duration,
    ) -> Result<Vec<AnalysisResult>, AnalysisError> {
        if let Some(analyzer) = self.async_analyzers.get(analyzer_name) {
            let analysis_future = analyzer.analyze_async(query, data);
            
            match tokio::time::timeout(timeout, analysis_future).await {
                Ok(result) => result,
                Err(_) => Err(AnalysisError::AnalysisFailed {
                    element_id: 0,
                    reason: format!("Analyzer '{}' timed out after {:?}", analyzer_name, timeout),
                }),
            }
        } else {
            Err(AnalysisError::AnalyzerNotFound {
                analyzer: analyzer_name.to_string(),
                available: self.async_analyzers.keys().cloned().collect(),
            })
        }
    }
}
```

### Task 3: Streaming Data Processing

Create a streaming analyzer for large datasets:

Create `oops-core/src/streaming.rs`:

```rust
//! Streaming analysis for large datasets

use crate::{AsyncAnalyzer, AnalysisQuery, AnalysisResult, AnalysisError, OsmData, OsmNode, OsmWay};
use async_trait::async_trait;
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use tokio::sync::mpsc;

/// Chunk of OSM data for streaming analysis
#[derive(Debug, Clone)]
pub struct OsmDataChunk {
    pub nodes: Vec<OsmNode>,
    pub ways: Vec<OsmWay>,
    pub chunk_id: usize,
    pub total_chunks: Option<usize>,
}

/// Stream-based analyzer for processing large datasets in chunks
#[async_trait]
pub trait StreamingAnalyzer: Send + Sync {
    fn name(&self) -> &str;
    
    /// Process a single chunk of data
    async fn analyze_chunk(
        &self,
        chunk: &OsmDataChunk,
        query: &AnalysisQuery,
    ) -> Result<Vec<AnalysisResult>, AnalysisError>;
    
    /// Combine results from multiple chunks (optional)
    async fn combine_results(
        &self,
        results: Vec<Vec<AnalysisResult>>,
    ) -> Result<Vec<AnalysisResult>, AnalysisError> {
        // Default implementation: flatten all results
        Ok(results.into_iter().flatten().collect())
    }
}

/// Streaming version of MissingMaxspeedAnalyzer
pub struct StreamingMaxspeedAnalyzer {
    inner: crate::MissingMaxspeedAnalyzer,
}

impl StreamingMaxspeedAnalyzer {
    pub fn new() -> Self {
        Self {
            inner: crate::MissingMaxspeedAnalyzer::new(),
        }
    }
}

#[async_trait]
impl StreamingAnalyzer for StreamingMaxspeedAnalyzer {
    fn name(&self) -> &str {
        "streaming_missing_maxspeed"
    }
    
    async fn analyze_chunk(
        &self,
        chunk: &OsmDataChunk,
        query: &AnalysisQuery,
    ) -> Result<Vec<AnalysisResult>, AnalysisError> {
        // Convert chunk to OsmData for existing analyzer
        let mut data = OsmData::new();
        
        for node in &chunk.nodes {
            data.add_node(node.clone());
        }
        
        for way in &chunk.ways {
            data.ways.insert(way.id, way.clone());
        }
        
        // Use existing analyzer logic
        self.inner.analyze(query, &data)
    }
}

/// Progress reporting for streaming analysis
pub struct AnalysisProgress {
    pub current_chunk: usize,
    pub total_chunks: Option<usize>,
    pub analyzer_name: String,
    pub results_so_far: usize,
}

/// Streaming analyzer runner
pub struct StreamingRunner<S: StreamingAnalyzer> {
    analyzer: S,
    progress_tx: Option<mpsc::UnboundedSender<AnalysisProgress>>,
}

impl<S: StreamingAnalyzer> StreamingRunner<S> {
    pub fn new(analyzer: S) -> Self {
        Self {
            analyzer,
            progress_tx: None,
        }
    }
    
    pub fn with_progress_reporting(mut self) -> (Self, mpsc::UnboundedReceiver<AnalysisProgress>) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.progress_tx = Some(tx);
        (self, rx)
    }
    
    pub async fn run_streaming<St>(
        &self,
        mut chunk_stream: St,
        query: AnalysisQuery,
    ) -> Result<Vec<AnalysisResult>, AnalysisError>
    where
        St: Stream<Item = OsmDataChunk> + Unpin,
    {
        let mut all_results = Vec::new();
        let mut chunk_results = Vec::new();
        let mut total_results = 0;
        
        while let Some(chunk) = chunk_stream.next().await {
            // Report progress
            if let Some(progress_tx) = &self.progress_tx {
                let progress = AnalysisProgress {
                    current_chunk: chunk.chunk_id,
                    total_chunks: chunk.total_chunks,
                    analyzer_name: self.analyzer.name().to_string(),
                    results_so_far: total_results,
                };
                
                if progress_tx.send(progress).is_err() {
                    eprintln!("Progress receiver dropped");
                }
            }
            
            // Analyze chunk
            let chunk_result = self.analyzer.analyze_chunk(&chunk, &query).await?;
            total_results += chunk_result.len();
            chunk_results.push(chunk_result);
        }
        
        // Combine all results
        self.analyzer.combine_results(chunk_results).await
    }
}
```

### Task 4: Update CLI for Async Operation

Update `oops-cli/src/main.rs` to use async analyzers:

```rust
use tokio::sync::mpsc;
use futures::stream::StreamExt;

async fn analyze_command(
    file: Option<PathBuf>,
    bbox: Option<String>, 
    analyzer: Option<String>,
    format: OutputFormat,
    verbose: bool,
) -> AnyhowResult<()> {
    // Set up async analyzer registry
    let mut registry = AsyncAnalyzerRegistry::new();
    registry.register_sync(MissingStreetNamesAnalyzer::new());
    registry.register_sync(MissingMaxspeedAnalyzer::new());
    
    // For demonstration, also add a streaming analyzer
    let streaming_analyzer = StreamingMaxspeedAnalyzer::new();
    
    if verbose {
        println!("Loading OSM data...");
    }

    let query_bbox = if let Some(bbox_str) = bbox {
        Some(parse_bbox(&bbox_str)
            .with_context(|| format!("Invalid bounding box format: '{}'", bbox_str))?)
    } else {
        None
    };

    let bbox = query_bbox.unwrap_or_else(|| BoundingBox::new(-180.0, -90.0, 180.0, 90.0));
    let query = AnalysisQuery::new(bbox);

    // Choose analysis method based on file size or user preference
    if let Some(pbf_path) = file {
        let file_size = std::fs::metadata(&pbf_path)
            .with_context(|| format!("Cannot access file: {:?}", pbf_path))?
            .len();
        
        if file_size > 100_000_000 {  // > 100MB, use streaming
            if verbose {
                println!("Large file detected, using streaming analysis...");
            }
            
            let results = analyze_streaming(&pbf_path, &query, streaming_analyzer, verbose).await?;
            output_results(vec![results], format)?;
        } else {
            // Regular analysis for smaller files
            let data = load_data_regular(&pbf_path, &query_bbox, verbose).await?;
            let results = analyze_concurrent(&registry, &query, &data, analyzer, verbose).await?;
            output_results(results, format)?;
        }
    } else {
        anyhow::bail!("File input is required for this lesson");
    }

    Ok(())
}

async fn load_data_regular(
    pbf_path: &PathBuf,
    query_bbox: &Option<BoundingBox>,
    verbose: bool,
) -> AnyhowResult<OsmData> {
    if verbose {
        println!("Reading PBF file: {:?}", pbf_path);
    }
    
    let reader = PbfReader::new();
    let filter = if let Some(bbox) = query_bbox {
        Some(PbfFilter::new().in_bbox(bbox))
    } else {
        None
    };
    
    let data = reader.read_file(pbf_path, filter)
        .await
        .with_context(|| format!("Failed to read PBF file: {:?}", pbf_path))?;
    
    if verbose {
        println!("Loaded {} nodes and {} ways", data.nodes.len(), data.ways.len());
    }
    
    Ok(data)
}

async fn analyze_concurrent(
    registry: &AsyncAnalyzerRegistry,
    query: &AnalysisQuery,
    data: &OsmData,
    analyzer_filter: Option<String>,
    verbose: bool,
) -> AnyhowResult<Vec<Vec<AnalysisResult>>> {
    if verbose {
        println!("Running analyzers concurrently...");
    }
    
    let start = std::time::Instant::now();
    
    let results = if let Some(analyzer_name) = analyzer_filter {
        // Run specific analyzer with timeout
        let timeout = std::time::Duration::from_secs(300);  // 5 minute timeout
        let result = registry.analyze_with_timeout(&analyzer_name, query, data, timeout)
            .await
            .with_context(|| format!("Analyzer '{}' failed", analyzer_name))?;
        vec![result]
    } else {
        // Run all analyzers concurrently  
        let concurrent_results = registry.analyze_all_concurrent(query, data).await;
        
        let mut successful_results = Vec::new();
        let mut errors = Vec::new();
        
        for (name, result) in concurrent_results {
            match result {
                Ok(analysis_results) => successful_results.push(analysis_results),
                Err(e) => errors.push(format!("{}: {}", name, e)),
            }
        }
        
        if !errors.is_empty() {
            eprintln!("Some analyzers failed:\n{}", errors.join("\n"));
        }
        
        successful_results
    };
    
    let duration = start.elapsed();
    if verbose {
        println!("Analysis completed in {:?}", duration);
    }
    
    Ok(results)
}

async fn analyze_streaming<A>(
    pbf_path: &PathBuf,
    query: &AnalysisQuery,
    analyzer: A,
    verbose: bool,
) -> AnyhowResult<Vec<AnalysisResult>>
where
    A: StreamingAnalyzer,
{
    let (runner, mut progress_rx) = StreamingRunner::new(analyzer).with_progress_reporting();
    
    // Create mock chunk stream for demonstration
    // In reality, this would read from the PBF file in chunks
    let chunk_stream = create_chunk_stream(pbf_path).await?;
    
    // Spawn progress reporting task
    let progress_handle = if verbose {
        Some(tokio::spawn(async move {
            while let Some(progress) = progress_rx.recv().await {
                if let Some(total) = progress.total_chunks {
                    println!(
                        "Processing chunk {}/{} - {} results found so far",
                        progress.current_chunk, total, progress.results_so_far
                    );
                } else {
                    println!(
                        "Processing chunk {} - {} results found so far",
                        progress.current_chunk, progress.results_so_far
                    );
                }
            }
        }))
    } else {
        None
    };
    
    let results = runner.run_streaming(chunk_stream, query.clone()).await?;
    
    if let Some(handle) = progress_handle {
        handle.abort();  // Stop progress reporting
    }
    
    Ok(results)
}

// Mock chunk stream creator - in real implementation would read PBF in chunks
async fn create_chunk_stream(
    _pbf_path: &PathBuf,
) -> AnyhowResult<impl Stream<Item = OsmDataChunk> + Unpin> {
    // This is a simplified example - real implementation would:
    // 1. Open PBF file with osmpbf crate
    // 2. Read in configurable chunk sizes
    // 3. Convert to OsmDataChunk objects
    
    use futures::stream;
    
    let chunks = vec![
        OsmDataChunk {
            nodes: vec![],
            ways: vec![],
            chunk_id: 1,
            total_chunks: Some(3),
        },
        OsmDataChunk {
            nodes: vec![],
            ways: vec![],
            chunk_id: 2,
            total_chunks: Some(3),
        },
        OsmDataChunk {
            nodes: vec![],
            ways: vec![],
            chunk_id: 3,
            total_chunks: Some(3),
        },
    ];
    
    Ok(stream::iter(chunks))
}
```

### Task 5: Async Error Handling and Cancellation

Add cancellation support and proper async error handling:

```rust
use tokio::sync::CancellationToken;
use tokio::select;

pub struct CancellableAnalysis {
    cancel_token: CancellationToken,
}

impl CancellableAnalysis {
    pub fn new() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
        }
    }
    
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }
    
    pub async fn run_with_cancellation<F, T>(
        &self,
        future: F,
    ) -> Result<T, AnalysisError>
    where
        F: Future<Output = Result<T, AnalysisError>>,
    {
        select! {
            result = future => result,
            _ = self.cancel_token.cancelled() => {
                Err(AnalysisError::AnalysisFailed {
                    element_id: 0,
                    reason: "Analysis was cancelled".to_string(),
                })
            }
        }
    }
}

// Usage example
async fn run_cancellable_analysis() -> AnyhowResult<()> {
    let analysis = CancellableAnalysis::new();
    let cancel_token = analysis.cancel_token.clone();
    
    // Set up cancellation after 10 seconds
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        cancel_token.cancel();
    });
    
    let results = analysis.run_with_cancellation(async {
        // Long-running analysis
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        Ok(vec![])
    }).await?;
    
    Ok(())
}
```

## Async Patterns and Best Practices

### 1. Structured Concurrency
```rust
use tokio::task::JoinSet;

async fn analyze_with_structured_concurrency(
    analyzers: Vec<Box<dyn AsyncAnalyzer>>,
    query: AnalysisQuery,
    data: OsmData,
) -> Result<Vec<Vec<AnalysisResult>>, AnalysisError> {
    let mut join_set = JoinSet::new();
    
    for analyzer in analyzers {
        let query = query.clone();
        let data = data.clone();
        
        join_set.spawn(async move {
            analyzer.analyze_async(&query, &data).await
        });
    }
    
    let mut results = Vec::new();
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(analysis_result) => results.push(analysis_result?),
            Err(join_error) => {
                return Err(AnalysisError::AnalysisFailed {
                    element_id: 0,
                    reason: format!("Task failed: {}", join_error),
                });
            }
        }
    }
    
    Ok(results)
}
```

### 2. Rate Limiting
```rust
use tokio::time::{interval, Duration};
use tokio::sync::Semaphore;

pub struct RateLimitedAnalyzer<A: AsyncAnalyzer> {
    inner: A,
    semaphore: Semaphore,
    interval: Duration,
}

impl<A: AsyncAnalyzer> RateLimitedAnalyzer<A> {
    pub fn new(analyzer: A, max_concurrent: usize, rate_limit: Duration) -> Self {
        Self {
            inner: analyzer,
            semaphore: Semaphore::new(max_concurrent),
            interval: rate_limit,
        }
    }
}

#[async_trait]
impl<A: AsyncAnalyzer + 'static> AsyncAnalyzer for RateLimitedAnalyzer<A> {
    fn name(&self) -> &str {
        self.inner.name()
    }
    
    async fn analyze_async(
        &self,
        query: &AnalysisQuery,
        data: &OsmData,
    ) -> Result<Vec<AnalysisResult>, AnalysisError> {
        // Acquire permit (blocks if too many concurrent requests)
        let _permit = self.semaphore.acquire().await.unwrap();
        
        // Rate limiting
        tokio::time::sleep(self.interval).await;
        
        self.inner.analyze_async(query, data).await
    }
}
```

### 3. Retry Logic
```rust
use backoff::{ExponentialBackoff, future::retry};

async fn analyze_with_retry<A: AsyncAnalyzer>(
    analyzer: &A,
    query: &AnalysisQuery,
    data: &OsmData,
) -> Result<Vec<AnalysisResult>, AnalysisError> {
    let backoff = ExponentialBackoff::default();
    
    retry(backoff, || async {
        match analyzer.analyze_async(query, data).await {
            Ok(results) => Ok(results),
            Err(AnalysisError::AnalysisFailed { .. }) => {
                // Retry transient failures
                Err(backoff::Error::transient("Analysis failed, retrying..."))
            }
            Err(e) => {
                // Don't retry permanent failures
                Err(backoff::Error::permanent(e))
            }
        }
    }).await
    .map_err(|e| match e {
        backoff::Error::Permanent(analysis_error) => analysis_error,
        backoff::Error::Transient { err, .. } => AnalysisError::AnalysisFailed {
            element_id: 0,
            reason: err.to_string(),
        },
    })
}
```

## Performance Considerations

### Memory Usage with Arc
```rust
use std::sync::Arc;

// Share data between async tasks efficiently
pub async fn analyze_shared_data(
    analyzers: Vec<Box<dyn AsyncAnalyzer>>,
    query: AnalysisQuery,
    data: Arc<OsmData>,  // Shared ownership
) -> HashMap<String, Result<Vec<AnalysisResult>, AnalysisError>> {
    let mut handles = Vec::new();
    
    for analyzer in analyzers {
        let query = query.clone();
        let data = Arc::clone(&data);  // Cheap reference counting
        
        let handle = tokio::spawn(async move {
            let name = analyzer.name().to_string();
            let result = analyzer.analyze_async(&query, &data).await;
            (name, result)
        });
        
        handles.push(handle);
    }
    
    // Collect results...
    HashMap::new()
}
```

### CPU-bound vs I/O-bound Tasks
```rust
// I/O-bound: use async
async fn fetch_remote_data() -> Result<OsmData, reqwest::Error> {
    let response = reqwest::get("https://overpass-api.de/api/interpreter")
        .await?
        .json()
        .await?;
    Ok(response)
}

// CPU-bound: use spawn_blocking
async fn cpu_intensive_analysis(data: OsmData) -> Result<Vec<AnalysisResult>, AnalysisError> {
    tokio::task::spawn_blocking(move || {
        // Heavy computation
        expensive_geometric_calculation(&data)
    }).await
    .map_err(|e| AnalysisError::AnalysisFailed {
        element_id: 0,
        reason: format!("Task join error: {}", e),
    })?
}
```

## Testing Async Code

```rust
#[cfg(test)]
mod async_tests {
    use super::*;
    use tokio::time::{timeout, Duration};
    
    #[tokio::test]
    async fn test_concurrent_analysis() {
        let registry = AsyncAnalyzerRegistry::new();
        registry.register_sync(MissingMaxspeedAnalyzer::new());
        
        let data = create_test_data();
        let query = AnalysisQuery::new(BoundingBox::new(0.0, 0.0, 1.0, 1.0));
        
        let results = registry.analyze_all_concurrent(&query, &data).await;
        
        assert_eq!(results.len(), 1);
        assert!(results.contains_key("missing_maxspeed"));
    }
    
    #[tokio::test]
    async fn test_analysis_timeout() {
        let registry = AsyncAnalyzerRegistry::new();
        registry.register_sync(SlowAnalyzer::new());
        
        let data = OsmData::new();
        let query = AnalysisQuery::new(BoundingBox::new(0.0, 0.0, 1.0, 1.0));
        
        let result = registry.analyze_with_timeout(
            "slow_analyzer",
            &query,
            &data,
            Duration::from_millis(100)  // Short timeout
        ).await;
        
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_streaming_analysis() {
        use futures::stream;
        
        let analyzer = StreamingMaxspeedAnalyzer::new();
        let runner = StreamingRunner::new(analyzer);
        
        let chunks = vec![
            OsmDataChunk {
                nodes: vec![],
                ways: vec![],
                chunk_id: 1,
                total_chunks: Some(1),
            }
        ];
        
        let chunk_stream = stream::iter(chunks);
        let query = AnalysisQuery::new(BoundingBox::new(0.0, 0.0, 1.0, 1.0));
        
        let results = runner.run_streaming(chunk_stream, query).await;
        assert!(results.is_ok());
    }
}
```

## Key Takeaways

1. **Async enables concurrency without threads** - Better resource utilization
2. **spawn_blocking for CPU-intensive work** - Avoid blocking the async runtime
3. **Arc for shared data** - Efficient sharing between async tasks  
4. **Structured concurrency with JoinSet** - Proper error handling and cleanup
5. **Timeouts and cancellation** - Prevent runaway tasks
6. **Streaming for large datasets** - Process data without loading everything in memory
7. **Rate limiting and retry logic** - Handle external service constraints

## Next Steps

With async programming mastered, [Lesson 7: Performance & Optimization](rust-lesson-07-performance.md) will dive deep into profiling, benchmarking, and optimizing both memory and CPU performance.

## Common Async Pitfalls

❌ **Blocking in async contexts**: Using `std::thread::sleep` instead of `tokio::time::sleep`  
✅ **Use async-aware primitives**: `tokio::time::sleep`, `tokio::fs::read`, etc.

❌ **Too many concurrent tasks**: Spawning unlimited tasks  
✅ **Use semaphores and rate limiting**: Control concurrency

❌ **Cloning large data**: `data.clone()` for every task  
✅ **Use Arc for shared ownership**: `Arc::clone(&data)`

❌ **Ignoring cancellation**: Long-running tasks without cancellation support  
✅ **Implement cancellation**: Use `CancellationToken` or `select!`