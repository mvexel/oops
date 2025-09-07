//! High-performance spatial indexing for OSM data
//!
//! This module provides efficient spatial indexing using R-trees and KD-trees
//! to replace O(n) linear searches with O(log n) indexed queries.

use crate::data::{OsmData, OsmWay};
use crate::spatial::{way_to_linestring, WayWithLineString};
use dashmap::DashMap;
use geo::{Point, LineString, BoundingRect, Intersects};
use rstar::{RTree, RTreeObject, AABB};
use std::sync::Arc;

/// Spatial index entry for ways
#[derive(Debug, Clone)]
pub struct WayIndex {
    pub way_id: i64,
    pub geometry: LineString,
    pub tags: std::collections::HashMap<String, String>,
}

impl RTreeObject for WayIndex {
    type Envelope = AABB<Point>;

    fn envelope(&self) -> Self::Envelope {
        self.geometry.envelope()
    }
}

/// Spatial index entry for nodes
#[derive(Debug, Clone)]
pub struct NodeIndex {
    pub node_id: i64,
    pub point: Point,
    pub tags: std::collections::HashMap<String, String>,
}

impl RTreeObject for NodeIndex {
    type Envelope = AABB<Point>;

    fn envelope(&self) -> Self::Envelope {
        AABB::from_corners(self.point, self.point)
    }
}

/// High-performance spatial index for OSM data
pub struct SpatialIndex {
    /// R-tree for way geometries - enables fast bounding box queries
    way_rtree: Arc<RTree<WayIndex>>,
    
    /// R-tree for node points - enables fast nearest neighbor queries
    node_rtree: Arc<RTree<NodeIndex>>,
    
    /// Specialized index for roads only (performance optimization)
    road_rtree: Arc<RTree<WayIndex>>,
    
    /// Cache for expensive spatial queries
    query_cache: DashMap<String, Vec<i64>>,
    
    /// Metadata
    way_count: usize,
    node_count: usize,
}

impl SpatialIndex {
    /// Build optimized spatial indexes from OSM data
    pub fn build(data: &OsmData) -> Self {
        use rayon::prelude::*;
        
        // Build way indexes in parallel
        let way_data: Vec<_> = data
            .ways
            .par_iter()
            .filter_map(|(_, way)| {
                way_to_linestring(way, data).map(|geometry| {
                    let entry = WayIndex {
                        way_id: way.id,
                        geometry,
                        tags: way.tags.clone(),
                    };
                    
                    let is_road = Self::is_road_way(way);
                    
                    (entry.clone(), 
                     if is_road { Some(entry) } else { None })
                })
            })
            .collect();
            
        let (way_entries, road_entries): (Vec<_>, Vec<_>) = 
            way_data.into_iter().fold((Vec::new(), Vec::new()), 
                |(mut ways, mut roads), (way, road)| {
                    ways.push(way);
                    if let Some(r) = road { roads.push(r); }
                    (ways, roads)
                });
            
        // Build node indexes in parallel
        let node_entries: Vec<NodeIndex> = data
            .nodes
            .par_iter()
            .map(|(_, node)| NodeIndex {
                node_id: node.id,
                point: Point::new(node.lon, node.lat),
                tags: node.tags.clone(),
            })
            .collect();
        
        // Build R-trees
        let way_rtree = Arc::new(RTree::bulk_load(way_entries));
        let node_rtree = Arc::new(RTree::bulk_load(node_entries));
        let road_rtree = Arc::new(RTree::bulk_load(road_entries));
        
        Self {
            way_rtree,
            node_rtree,
            road_rtree,
            query_cache: DashMap::new(),
            way_count: data.ways.len(),
            node_count: data.nodes.len(),
        }
    }
    
    /// Find ways within distance using tag-based filtering - generic O(log n) query
    pub fn find_nearby_ways_by_tags<F>(&self, target_way: &OsmWay, data: &OsmData, max_distance: f64, tag_filter: F) -> Vec<WayWithLineString>
    where
        F: Fn(&std::collections::HashMap<String, String>) -> bool,
    {
        let Some(target_geometry) = way_to_linestring(target_way, data) else {
            return Vec::new();
        };
        
        // Create search envelope  
        let Some(bbox) = target_geometry.bounding_rect() else {
            return Vec::new();
        };
        
        let buffer_degrees = max_distance / 111_320.0;
        let search_envelope = AABB::from_corners(
            Point::new(bbox.min().x - buffer_degrees, bbox.min().y - buffer_degrees),
            Point::new(bbox.max().x + buffer_degrees, bbox.max().y + buffer_degrees),
        );
        
        // Fast spatial query using general way R-tree
        let candidates: Vec<_> = self
            .way_rtree
            .locate_in_envelope_intersecting(&search_envelope)
            .filter(|way_entry| {
                // Apply tag filter and distance check
                tag_filter(&way_entry.tags) && 
                self.calculate_way_distance(&target_geometry, &way_entry.geometry) <= max_distance
            })
            .map(|way_entry| way_entry.way_id)
            .collect();
        
        self.ids_to_ways_with_linestring(candidates, data)
    }
    
    /// Find intersecting ways for intersection detection - O(log n)
    pub fn find_intersecting_ways(&self, target_way: &OsmWay, data: &OsmData, max_distance: f64) -> Vec<i64> {
        let cache_key = format!("intersect_{}_{}", target_way.id, max_distance as i32);
        
        if let Some(cached_ids) = self.query_cache.get(&cache_key) {
            return cached_ids.clone();
        }
        
        let Some(target_geometry) = way_to_linestring(target_way, data) else {
            return Vec::new();
        };
        
        let Some(bbox) = target_geometry.bounding_rect() else {
            return Vec::new();
        };
        
        let buffer_degrees = max_distance / 111_320.0;
        let search_envelope = AABB::from_corners(
            Point::new(bbox.min().x - buffer_degrees, bbox.min().y - buffer_degrees),
            Point::new(bbox.max().x + buffer_degrees, bbox.max().y + buffer_degrees),
        );
        
        let candidates: Vec<_> = self
            .road_rtree
            .locate_in_envelope_intersecting(&search_envelope)
            .filter(|candidate| {
                candidate.way_id != target_way.id &&
                self.ways_intersect(&target_geometry, &candidate.geometry)
            })
            .map(|way| way.way_id)
            .collect();
            
        self.query_cache.insert(cache_key, candidates.clone());
        candidates
    }
    
    /// Find nodes within radius - O(log n)  
    pub fn find_nodes_within_radius(&self, center: Point, radius_meters: f64) -> Vec<i64> {
        let cache_key = format!("nodes_{}_{}", 
            (center.x() * 1000000.0) as i32, 
            (center.y() * 1000000.0) as i32);
            
        if let Some(cached_ids) = self.query_cache.get(&cache_key) {
            return cached_ids.clone();
        }
        
        let radius_degrees = radius_meters / 111_320.0;
        let search_envelope = AABB::from_corners(
            Point::new(center.x() - radius_degrees, center.y() - radius_degrees),
            Point::new(center.x() + radius_degrees, center.y() + radius_degrees),
        );
        
        use geo::HaversineDistance;
        
        let candidates: Vec<_> = self
            .node_rtree
            .locate_in_envelope_intersecting(&search_envelope)
            .filter(|node| center.haversine_distance(&node.point) <= radius_meters)
            .map(|node| node.node_id)
            .collect();
            
        self.query_cache.insert(cache_key, candidates.clone());
        candidates
    }
    
    /// Get spatial index statistics
    pub fn stats(&self) -> SpatialIndexStats {
        SpatialIndexStats {
            way_count: self.way_count,
            node_count: self.node_count,
            road_count: self.road_rtree.size(),
            cache_size: self.query_cache.len(),
        }
    }
    
    /// Clear query cache
    pub fn clear_cache(&self) {
        self.query_cache.clear();
    }
    
    // Helper methods
    
    fn is_road_way(way: &OsmWay) -> bool {
        way.tags.get("highway").map_or(false, |h| {
            matches!(h.as_str(), 
                "primary" | "secondary" | "tertiary" | "unclassified" | "residential" | 
                "trunk" | "motorway" | "primary_link" | "secondary_link" | 
                "tertiary_link" | "trunk_link" | "motorway_link"
            )
        })
    }
    
    fn calculate_way_distance(&self, way1: &LineString, way2: &LineString) -> f64 {
        use crate::spatial::calculate_linestring_to_linestring_distance;
        calculate_linestring_to_linestring_distance(way1, way2)
    }
    
    fn ways_intersect(&self, way1: &LineString, way2: &LineString) -> bool {
        way1.intersects(way2)
    }
    
    fn ids_to_ways_with_linestring(&self, way_ids: Vec<i64>, data: &OsmData) -> Vec<WayWithLineString> {
        way_ids
            .into_iter()
            .filter_map(|id| {
                data.ways.get(&id).and_then(|way| {
                    way_to_linestring(way, data).map(|linestring| WayWithLineString {
                        way: way.clone(),
                        linestring,
                    })
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct SpatialIndexStats {
    pub way_count: usize,
    pub node_count: usize,
    pub road_count: usize,
    pub cache_size: usize,
}

