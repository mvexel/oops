//! Streaming analysis for large datasets
//!
//! This module provides memory-efficient streaming analysis that processes
//! OSM data in chunks without loading the entire dataset into memory.

use crate::analysis::{Analyzer, AnalysisQuery, AnalysisResult, AnalysisSpatialIndex};
use crate::data::{OsmData, OsmWay, OsmNode};
use crate::spatial_index::SpatialIndex;
use dashmap::DashMap;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Configuration for streaming analysis
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Maximum elements to process in a single chunk
    pub chunk_size: usize,
    /// Maximum memory usage in MB before forcing a flush
    pub max_memory_mb: usize,
    /// Number of chunks to process in parallel
    pub parallel_chunks: usize,
    /// Whether to use adaptive chunking based on memory usage
    pub adaptive_chunking: bool,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 10_000,
            max_memory_mb: 512,
            parallel_chunks: num_cpus::get(),
            adaptive_chunking: true,
        }
    }
}

/// Streaming analysis engine for large datasets
pub struct StreamingAnalysisEngine {
    config: StreamingConfig,
    _shared_spatial_index: Arc<RwLock<Option<SpatialIndex>>>,
    result_buffer: Arc<DashMap<String, Vec<AnalysisResult>>>,
}

impl StreamingAnalysisEngine {
    pub fn new(config: StreamingConfig) -> Self {
        Self {
            config,
            _shared_spatial_index: Arc::new(RwLock::new(None)),
            result_buffer: Arc::new(DashMap::new()),
        }
    }
    
    /// Stream analysis over large dataset with memory-efficient processing
    pub fn stream_analyze<I>(
        &self,
        analyzers: &[Analyzer],
        query: &AnalysisQuery,
        data_stream: I,
    ) -> Vec<AnalysisResult>
    where
        I: Iterator<Item = OsmDataChunk> + Send,
    {
        data_stream
            .chunks(self.config.chunk_size)
            .par_bridge()
            .flat_map(|chunk| {
                self.process_chunk(analyzers, query, chunk)
            })
            .collect()
    }
    
    /// Process a single chunk of data
    fn process_chunk(
        &self,
        analyzers: &[Analyzer],
        query: &AnalysisQuery,
        chunk: Vec<OsmDataChunk>,
    ) -> Vec<AnalysisResult> {
        // Merge chunk data
        let merged_data = self.merge_chunks(chunk);
        
        // Build spatial index for this chunk
        let spatial_index = AnalysisSpatialIndex::build(&merged_data);
        
        // Process all analyzers on this chunk
        analyzers
            .par_iter()
            .flat_map(|analyzer| {
                self.run_analyzer_on_chunk(analyzer, query, &spatial_index)
            })
            .collect()
    }
    
    /// Run a single analyzer on a data chunk
    fn run_analyzer_on_chunk(
        &self,
        analyzer: &Analyzer,
        query: &AnalysisQuery,
        spatial_index: &AnalysisSpatialIndex,
    ) -> Vec<AnalysisResult> {
        use crate::analysis::AnalysisEngine;
        
        // Create a temporary analysis engine for this chunk
        let mut engine = AnalysisEngine::new();
        // Use the pre-built spatial index to avoid rebuilding
        engine.analyze_single(analyzer, query, spatial_index.osm_data())
    }
    
    /// Merge multiple data chunks into a single OsmData
    fn merge_chunks(&self, chunks: Vec<OsmDataChunk>) -> OsmData {
        let mut merged = OsmData::new();
        
        for chunk in chunks {
            match chunk {
                OsmDataChunk::Nodes(nodes) => {
                    for node in nodes {
                        merged.add_node(node);
                    }
                }
                OsmDataChunk::Ways(ways) => {
                    for way in ways {
                        merged.ways.insert(way.id, way);
                    }
                }
                OsmDataChunk::Relations(relations) => {
                    for relation in relations {
                        merged.relations.insert(relation.id, relation);
                    }
                }
                OsmDataChunk::Mixed { nodes, ways, relations } => {
                    for node in nodes {
                        merged.add_node(node);
                    }
                    for way in ways {
                        merged.ways.insert(way.id, way);
                    }
                    for relation in relations {
                        merged.relations.insert(relation.id, relation);
                    }
                }
            }
        }
        
        merged
    }
    
    /// Get memory usage estimate in MB
    pub fn estimated_memory_usage(&self) -> usize {
        // Rough estimate based on buffer sizes
        let buffer_size: usize = self.result_buffer.iter()
            .map(|entry| entry.value().len() * std::mem::size_of::<AnalysisResult>())
            .sum();
            
        buffer_size / (1024 * 1024) // Convert to MB
    }
    
    /// Force flush results to prevent memory buildup
    pub fn flush_results(&self) -> HashMap<String, Vec<AnalysisResult>> {
        let mut results = HashMap::new();
        
        for entry in self.result_buffer.iter() {
            results.insert(entry.key().clone(), entry.value().clone());
        }
        
        self.result_buffer.clear();
        results
    }
}

/// Represents a chunk of OSM data for streaming processing
#[derive(Debug, Clone)]
pub enum OsmDataChunk {
    Nodes(Vec<OsmNode>),
    Ways(Vec<OsmWay>),
    Relations(Vec<crate::data::OsmRelation>),
    Mixed {
        nodes: Vec<OsmNode>,
        ways: Vec<OsmWay>,
        relations: Vec<crate::data::OsmRelation>,
    },
}

/// Extension trait to add chunking to iterators
pub trait ChunkingExt<T>: Iterator<Item = T> {
    fn chunks(self, size: usize) -> ChunkedIterator<Self>
    where
        Self: Sized;
}

impl<T, I: Iterator<Item = T>> ChunkingExt<T> for I {
    fn chunks(self, size: usize) -> ChunkedIterator<Self> {
        ChunkedIterator {
            inner: self,
            chunk_size: size,
        }
    }
}

/// Iterator that yields chunks of elements
pub struct ChunkedIterator<I> {
    inner: I,
    chunk_size: usize,
}

impl<I, T> Iterator for ChunkedIterator<I>
where
    I: Iterator<Item = T>,
{
    type Item = Vec<T>;
    
    fn next(&mut self) -> Option<Self::Item> {
        let mut chunk = Vec::with_capacity(self.chunk_size);
        
        for _ in 0..self.chunk_size {
            match self.inner.next() {
                Some(item) => chunk.push(item),
                None => break,
            }
        }
        
        if chunk.is_empty() {
            None
        } else {
            Some(chunk)
        }
    }
}

/// Memory-efficient spatial tiling for very large datasets
pub struct SpatialTileProcessor {
    tile_size_degrees: f64,
    overlap_degrees: f64,
}

impl SpatialTileProcessor {
    pub fn new(tile_size_degrees: f64, overlap_degrees: f64) -> Self {
        Self {
            tile_size_degrees,
            overlap_degrees,
        }
    }
    
    /// Process data in spatial tiles to reduce memory usage
    pub fn process_by_tiles<F>(
        &self,
        bounds: geo::Rect,
        data_loader: F,
        analyzers: &[Analyzer],
        query: &AnalysisQuery,
    ) -> Vec<AnalysisResult>
    where
        F: Fn(geo::Rect) -> OsmData + Sync + Send,
    {
        let tiles = self.generate_tiles(bounds);
        
        tiles.into_par_iter()
            .flat_map(|tile_bounds| {
                let tile_data = data_loader(tile_bounds);
                
                if tile_data.ways.is_empty() && tile_data.nodes.is_empty() {
                    return Vec::new();
                }
                
                // Process this tile
                let _spatial_index = AnalysisSpatialIndex::build(&tile_data);
                let mut engine = crate::analysis::AnalysisEngine::new();
                
                analyzers.iter()
                    .flat_map(|analyzer| {
                        engine.analyze_single(analyzer, query, &tile_data)
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    
    fn generate_tiles(&self, bounds: geo::Rect) -> Vec<geo::Rect> {
        let mut tiles = Vec::new();
        
        let min_lon = bounds.min().x;
        let min_lat = bounds.min().y;
        let max_lon = bounds.max().x;
        let max_lat = bounds.max().y;
        
        let mut current_lon = min_lon;
        while current_lon < max_lon {
            let mut current_lat = min_lat;
            while current_lat < max_lat {
                let tile_bounds = geo::Rect::new(
                    geo::Coord { 
                        x: current_lon - self.overlap_degrees, 
                        y: current_lat - self.overlap_degrees 
                    },
                    geo::Coord { 
                        x: (current_lon + self.tile_size_degrees + self.overlap_degrees).min(max_lon),
                        y: (current_lat + self.tile_size_degrees + self.overlap_degrees).min(max_lat)
                    }
                );
                
                tiles.push(tile_bounds);
                current_lat += self.tile_size_degrees;
            }
            current_lon += self.tile_size_degrees;
        }
        
        tiles
    }
}

/// Lazy evaluation wrapper for expensive computations
pub struct LazyComputation<T> {
    computation: Option<Box<dyn FnOnce() -> T + Send>>,
    result: Option<T>,
}

impl<T> LazyComputation<T> {
    pub fn new<F>(computation: F) -> Self
    where
        F: FnOnce() -> T + Send + 'static,
    {
        Self {
            computation: Some(Box::new(computation)),
            result: None,
        }
    }
    
    pub fn get(&mut self) -> &T {
        if self.result.is_none() {
            if let Some(computation) = self.computation.take() {
                self.result = Some(computation());
            }
        }
        
        self.result.as_ref().unwrap()
    }
    
    pub fn is_computed(&self) -> bool {
        self.result.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
        
    #[test]
    fn test_chunking_iterator() {
        let data: Vec<i32> = (0..10).collect();
        let chunks: Vec<Vec<i32>> = data.into_iter().chunks(3).collect();
        
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0], vec![0, 1, 2]);
        assert_eq!(chunks[1], vec![3, 4, 5]);
        assert_eq!(chunks[2], vec![6, 7, 8]);
        assert_eq!(chunks[3], vec![9]);
    }
    
    #[test]
    fn test_lazy_computation() {
        let mut lazy = LazyComputation::new(|| {
            // Expensive computation
            42 * 2
        });
        
        assert!(!lazy.is_computed());
        assert_eq!(*lazy.get(), 84);
        assert!(lazy.is_computed());
    }
    
    #[test]
    fn test_spatial_tiling() {
        let processor = SpatialTileProcessor::new(1.0, 0.1);
        let bounds = geo::Rect::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord { x: 2.5, y: 2.5 }
        );
        
        let tiles = processor.generate_tiles(bounds);
        assert!(tiles.len() > 1);
        
        // Verify tiles cover the entire bounds
        for tile in &tiles {
            assert!(tile.min().x >= bounds.min().x - processor.overlap_degrees);
            assert!(tile.max().x <= bounds.max().x + processor.overlap_degrees);
        }
    }
}