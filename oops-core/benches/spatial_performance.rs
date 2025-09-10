use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use oops_core::*;

/// Generate synthetic OSM data for performance testing
fn generate_test_data(way_count: usize, node_count: usize) -> OsmData {
    let mut data = OsmData::new();
    
    // Generate nodes in a grid pattern
    for i in 0..node_count {
        let lat = 40.0 + (i / 100) as f64 * 0.01;
        let lon = -111.0 + (i % 100) as f64 * 0.01;
        let mut node = OsmNode::new(i as i64, lon, lat);
        
        // Add some tags to some nodes  
        if i % 10 == 0 {
            node.tags.insert("highway".into(), "crossing".into());
        }
        
        data.add_node(node);
    }
    
    // Generate ways connecting nodes
    for i in 0..way_count {
        let start_node = (i * 2) % node_count;
        let end_node = (start_node + 1) % node_count;
        
        let mut way = OsmWay::new(i as i64, vec![start_node as i64, end_node as i64]);
        
        // Add different highway types
        let highway_type = match i % 5 {
            0 => "residential",
            1 => "primary",
            2 => "secondary", 
            3 => "footway",
            4 => "tertiary",
            _ => "unclassified",
        };
        way.tags.insert("highway".into(), highway_type.into());
        
        // Make some footways into sidewalks
        if highway_type == "footway" && i % 3 == 0 {
            way.tags.insert("footway".into(), "sidewalk".into());
        }
        
        data.ways.insert(way.id, way);
    }
    
    data
}

/// Benchmark spatial index construction
fn bench_spatial_index_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_index_construction");
    
    for size in [100, 500, 1000, 5000].iter() {
        let data = generate_test_data(*size, *size * 2);
        
        group.bench_with_input(BenchmarkId::new("build_index", size), size, |b, _| {
            b.iter(|| {
                let _index = SpatialIndex::build(black_box(&data));
            });
        });
    }
    group.finish();
}

/// Benchmark spatial queries - old vs new approach
fn bench_spatial_queries(c: &mut Criterion) {
    let mut group = c.benchmark_group("spatial_queries");
    let data = generate_test_data(2000, 4000);
    let spatial_index = SpatialIndex::build(&data);
    
    // Get a sample way to query against
    let sample_way = data.ways.values().next().unwrap();
    
    group.bench_function("optimized_nearby_sidewalks", |b| {
        b.iter(|| {
            let _results = spatial_index.find_nearby_ways_by_tags(
                black_box(sample_way),
                black_box(&data),
                black_box(50.0),
                |tags| {
                    tags.get("highway") == Some(&"footway".to_string()) &&
                    tags.get("footway") == Some(&"sidewalk".to_string())
                }
            );
        });
    });
    
    group.bench_function("optimized_intersecting_ways", |b| {
        b.iter(|| {
            let _results = spatial_index.find_intersecting_ways(
                black_box(sample_way),
                black_box(&data), 
                black_box(100.0)
            );
        });
    });
    
    group.finish();
}

/// Benchmark analysis engine performance
fn bench_analysis_engine(c: &mut Criterion) {
    let mut group = c.benchmark_group("analysis_engine");
    let data = generate_test_data(1000, 2000);
    
    // Create a simple test analyzer
    let analyzer = Analyzer::new(
        "test_analyzer",
        "1.0.0", 
        "Test analyzer for benchmarking",
        Box::new(WaySelector::new().highway_types(vec!["primary", "secondary"])),
        Box::new(NoContext),
        Box::new(MissingTagDetector::new("maxspeed")),
    );
    
    let bbox = BoundingBox::new(-111.1, 39.9, -110.9, 40.1);
    let query = AnalysisQuery::new(bbox);
    
    group.bench_function("parallel_analysis", |b| {
        b.iter(|| {
            let mut engine = AnalysisEngine::new();
            let _results = engine.analyze_single(
                black_box(&analyzer),
                black_box(&query),
                black_box(&data)
            );
        });
    });
    
    group.finish();
}

/// Benchmark SIMD geometry operations  
fn bench_simd_geometry(c: &mut Criterion) {
    use geo::Point;
    
    let mut group = c.benchmark_group("simd_geometry");
    let center = Point::new(-111.0, 40.0);
    let points: Vec<Point> = (0..1000)
        .map(|i| Point::new(-111.0 + i as f64 * 0.001, 40.0 + i as f64 * 0.001))
        .collect();
        
    group.bench_function("simd_point_distances", |b| {
        b.iter(|| {
            let _distances = SIMDGeometry::point_to_points_distance(
                black_box(center),
                black_box(&points)
            );
        });
    });
    
    // Benchmark parallel filtering
    group.bench_function("simd_filter_by_distance", |b| {
        b.iter(|| {
            let _filtered = SIMDSpatialOps::filter_points_by_distance(
                black_box(center),
                black_box(&points),
                black_box(1000.0)
            );
        });
    });
    
    group.finish();
}

/// Benchmark memory usage and cache performance
fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");
    
    for size in [500, 1000, 2000].iter() {
        let data = generate_test_data(*size, *size * 2);
        
        group.bench_with_input(BenchmarkId::new("data_clone", size), size, |b, _| {
            b.iter(|| {
                let _cloned = black_box(&data).clone();
            });
        });
        
        let spatial_index = SpatialIndex::build(&data);
        group.bench_with_input(BenchmarkId::new("cache_performance", size), size, |b, _| {
            b.iter(|| {
                // This should hit the cache on subsequent runs
                let sample_way = data.ways.values().next().unwrap();
                let _results = spatial_index.find_nearby_ways_by_tags(
                    sample_way,
                    &data,
                    50.0,
                    |tags| tags.get("highway") == Some(&"footway".to_string())
                );
            });
        });
    }
    
    group.finish();
}

criterion_group!(
    spatial_benches,
    bench_spatial_index_construction,
    bench_spatial_queries,
    bench_analysis_engine,
    bench_simd_geometry,
    bench_memory_efficiency
);
criterion_main!(spatial_benches);