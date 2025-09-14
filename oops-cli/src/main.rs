use clap::{Parser, Subcommand};
use geo::{Euclidean, Geodesic, InterpolatableLine, Length};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value as GeoValue};
use oops_analyzers::get_all_analyzers;
use oops_core::{
    AnalysisQuery, AnalysisResult, AnalyzerRegistry, BoundingBox, ElementType, OsmData,
};
use oops_ingest::{OverpassClient, PbfFilter, PbfReader};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "oops")]
#[command(about = "OSM Object Problem Scanner - Analyze OpenStreetMap data for quality issues")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze OSM data for quality issues
    Analyze {
        /// Input PBF file path
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Bounding box as "min_lon,min_lat,max_lon,max_lat"
        #[arg(short, long)]
        bbox: Option<String>,

        /// Overpass query (requires bbox)
        #[arg(short, long)]
        query: Option<String>,

        /// Analyzer to run (default: all)
        #[arg(short, long)]
        analyzer: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,

        /// Verbose output
        #[arg(short, long)]
        _verbose: bool,

        /// Debug specific element by ID
        #[arg(long)]
        debug_element: Option<i64>,

        /// Enable validation mode (compare against ground truth tags)
        #[arg(long)]
        validate: bool,

        /// Distance for clustering intersections (meters)
        #[arg(long, default_value = "50.0")]
        cluster_distance: f64,
    },
    /// List available analyzers
    List,
}

#[derive(clap::ValueEnum, Clone)]
enum OutputFormat {
    Json,
    Csv,
    Summary,
    Geojson,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze {
            file,
            bbox,
            query,
            analyzer,
            format,
            _verbose,
            debug_element,
            validate,
            cluster_distance,
        } => {
            analyze_command(
                file,
                bbox,
                query,
                analyzer,
                format,
                _verbose,
                debug_element,
                validate,
                cluster_distance,
            )
            .await?;
        }
        Commands::List => {
            list_analyzers();
        }
    }

    Ok(())
}

/// Create a new analyzer registry and populate it with all available analyzers.
fn create_and_populate_registry(cluster_distance: f64) -> AnalyzerRegistry {
    let mut registry = AnalyzerRegistry::new();
    for analyzer in get_all_analyzers(cluster_distance) {
        registry.register(analyzer);
    }
    registry
}

/// Create a new analyzer registry in validation mode
fn create_validation_registry(cluster_distance: f64) -> AnalyzerRegistry {
    // For now, just return the same as normal mode
    // TODO: Implement validation-specific analyzers when needed
    create_and_populate_registry(cluster_distance)
}

async fn analyze_command(
    file: Option<PathBuf>,
    bbox: Option<String>,
    query: Option<String>,
    analyzer: Option<String>,
    format: OutputFormat,
    _verbose: bool,
    debug_element: Option<i64>,
    validate: bool,
    cluster_distance: f64,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    // Parse bounding box if provided
    let query_bbox = if let Some(bbox_str) = bbox {
        Some(parse_bbox(&bbox_str)?)
    } else {
        None
    };

    // Load data from file or Overpass API
    eprintln!("Loading OSM data...");
    let data = if let Some(pbf_path) = file {
        eprintln!("Reading PBF file: {:?}", pbf_path);
        let reader = PbfReader::new();
        let filter = query_bbox
            .as_ref()
            .map(|bbox| PbfFilter::new().in_bbox(bbox));
        reader.read_file(&pbf_path, filter).await?
    } else if let (Some(bbox), Some(overpass_query)) = (&query_bbox, &query) {
        eprintln!("Fetching data from Overpass API...");
        let client = OverpassClient::new();
        client.fetch_in_bbox(bbox, overpass_query).await?
    } else {
        return Err("Either --file or --bbox with --query must be provided".into());
    };

    eprintln!(
        "Loaded {} nodes and {} ways",
        data.nodes.len(),
        data.ways.len()
    );

    // Debug specific element if requested
    if let Some(element_id) = debug_element {
        debug_element_analysis(element_id, &data).await?;
        return Ok(())
    }

    // Set up analyzer registry
    let registry = if validate {
        create_validation_registry(cluster_distance)
    } else {
        create_and_populate_registry(cluster_distance)
    };

    // Run analysis
    let bbox = query_bbox.unwrap_or_else(|| BoundingBox::new(-180.0, -90.0, 180.0, 90.0));
    let analysis_query = AnalysisQuery::new(bbox);

    let results: Vec<(String, Vec<AnalysisResult>)> = if let Some(analyzer_name) = analyzer {
        // Run specific analyzer
        eprintln!("Running analyzer: {}", analyzer_name);
        let result = registry.analyze(&analyzer_name, &analysis_query, &data)?;
        let finding_count: usize = result.iter().map(|r| r.findings.len()).sum();
        eprintln!("Completed {} ({} findings)", analyzer_name, finding_count);

        vec![(analyzer_name.clone(), result)]
    } else {
        // Run all analyzers
        let analyzer_names: Vec<String> = registry.list().iter().map(|s| s.to_string()).collect();
        eprintln!("Running {} analyzers...", analyzer_names.len());

        let mut results = Vec::new();

        for (idx, analyzer_name) in analyzer_names.iter().enumerate() {
            eprintln!(
                "Running {} ({}/{})",
                analyzer_name,
                idx + 1,
                analyzer_names.len()
            );
            let result = registry.analyze(analyzer_name, &analysis_query, &data)?;
            let finding_count: usize = result.iter().map(|r| r.findings.len()).sum();
            eprintln!("Completed {} ({} findings)", analyzer_name, finding_count);

            results.push((analyzer_name.clone(), result));
        }

        let total_findings: usize = results
            .iter()
            .flat_map(|(_, results)| results)
            .map(|r| r.findings.len())
            .sum();

        eprintln!("Analysis complete: {} total findings", total_findings);

        results
    };

    // Output results
    output_results(results, format, &data)?;

    Ok(())
}

fn list_analyzers() {
    let registry = create_and_populate_registry(50.0); // Use default for listing

    eprintln!("Available analyzers:");
    for info in registry.get_all_info() {
        eprintln!("  {} (v{}) - {}", info.name, info.version, info.description);
    }
}

fn parse_bbox(bbox_str: &str) -> std::result::Result<BoundingBox, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = bbox_str.split(',').collect();
    if parts.len() != 4 {
        return Err("Bbox must be in format: min_lon,min_lat,max_lon,max_lat".into());
    }

    let min_lon: f64 = parts[0].parse()?;
    let min_lat: f64 = parts[1].parse()?;
    let max_lon: f64 = parts[2].parse()?;
    let max_lat: f64 = parts[3].parse()?;

    Ok(BoundingBox::new(min_lon, min_lat, max_lon, max_lat))
}

fn output_results(
    results: Vec<(String, Vec<AnalysisResult>)>, // Note: The structure now includes the analyzer name
    format: OutputFormat,
    osm_data: &OsmData,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Json => {
            let flattened: Vec<&AnalysisResult> = results.iter().flat_map(|(_, r)| r).collect();
            println!("{}", serde_json::to_string_pretty(&flattened)?);
        }
        OutputFormat::Summary => {
            let total_findings: usize = results
                .iter()
                .flat_map(|(_, r)| r)
                .map(|r| r.findings.len())
                .sum();

            eprintln!("Analysis Summary:");
            eprintln!("  Issues found: {}", total_findings);

            if total_findings > 0 {
                eprintln!("\nIssues by Analyzer:");
                for (analyzer_name, analyzer_results) in &results {
                    if !analyzer_results.is_empty() {
                        eprintln!("  - {}: {} issues", analyzer_name, analyzer_results.len());
                    }
                }
            }
        }
        OutputFormat::Csv => {
            println!("element_id,analyzer,severity,message,context");
            for (analyzer_name, analyzer_results) in results {
                for result in analyzer_results {
                    for finding in &result.findings {
                        let context = serde_json::to_string(&finding.context).unwrap_or_default();
                        println!(
                            "{},-{},{:?},{},\"{}\"",
                            result.element_id,
                            analyzer_name, // Use the correct analyzer name
                            finding.severity,
                            finding.message.as_ref().unwrap_or(&"No message".into()),
                            context
                        );
                    }
                }
            }
        }
        OutputFormat::Geojson => {
            let mut features = Vec::new();

            for (analyzer_name, analyzer_results) in results {
                for result in analyzer_results {
                    if let Some(geometry) = create_geometry_from_element(
                        result.element_id,
                        &result.element_type,
                        osm_data,
                    ) {
                        for finding in &result.findings {
                            let mut properties = HashMap::new();
                            properties.insert(
                                "element_id".to_string(),
                                serde_json::Value::Number(result.element_id.into()),
                            );
                            properties.insert(
                                "analyzer".to_string(),
                                serde_json::Value::String(analyzer_name.clone()),
                            );
                            properties.insert(
                                "severity".to_string(),
                                serde_json::Value::String(format!("{:?}", finding.severity)),
                            );
                            if let Some(message) = &finding.message {
                                properties.insert(
                                    "message".to_string(),
                                    serde_json::Value::String(message.clone()),
                                );
                            }

                            // Add context fields
                            for (key, value) in &finding.context {
                                properties.insert(
                                    format!("context_{}", key),
                                    serde_json::Value::String(value.clone()),
                                );
                            }

                            let feature = Feature {
                                bbox: None,
                                geometry: Some(geometry.clone()),
                                id: Some(geojson::feature::Id::Number(result.element_id.into())),
                                properties: Some(serde_json::Map::from_iter(properties)),
                                foreign_members: None,
                            };
                            features.push(feature);
                        }
                    }
                }
            }

            let feature_collection = FeatureCollection {
                bbox: None,
                features,
                foreign_members: None,
            };

            let geojson = GeoJson::FeatureCollection(feature_collection);
            println!("{}", serde_json::to_string_pretty(&geojson)?);
        }
    }

    Ok(())
}

/// Create GeoJSON geometry from an OSM element
fn create_geometry_from_element(
    element_id: i64,
    element_type: &ElementType,
    osm_data: &OsmData,
) -> Option<Geometry> {
    match element_type {
        ElementType::Node => osm_data
            .get_node(element_id)
            .map(|node| Geometry::new(GeoValue::Point(vec![node.lon, node.lat]))),
        ElementType::Way => {
            if let Some(way) = osm_data.ways.get(&element_id) {
                let coords: Option<Vec<Vec<f64>>> = way
                    .nodes
                    .iter()
                    .map(|&node_id| {
                        osm_data
                            .get_node(node_id)
                            .map(|node| vec![node.lon, node.lat])
                    })
                    .collect();

                if let Some(coords) = coords {
                    // Check if it's a closed way (polygon)
                    if way.is_closed() && coords.len() >= 4 {
                        Some(Geometry::new(GeoValue::Polygon(vec![coords])))
                    } else {
                        Some(Geometry::new(GeoValue::LineString(coords)))
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        ElementType::Relation => {
            // For relations, we'll create a point at the centroid for now
            // A more sophisticated implementation could handle multipolygons
            if let Some(relation) = osm_data.relations.get(&element_id) {
                // Calculate centroid from member ways/nodes
                let mut lons = Vec::new();
                let mut lats = Vec::new();

                for member in &relation.members {
                    match member.member_type {
                        oops_core::MemberType::Node => {
                            if let Some(node) = osm_data.get_node(member.member_id) {
                                lons.push(node.lon);
                                lats.push(node.lat);
                            }
                        }
                        oops_core::MemberType::Way => {
                            if let Some(way) = osm_data.ways.get(&member.member_id) {
                                for &node_id in &way.nodes {
                                    if let Some(node) = osm_data.get_node(node_id) {
                                        lons.push(node.lon);
                                        lats.push(node.lat);
                                    }
                                }
                            }
                        }
                        oops_core::MemberType::Relation => {
                            // Skip nested relations for simplicity
                        }
                    }
                }

                if !lons.is_empty() && !lats.is_empty() {
                    let centroid_lon = lons.iter().sum::<f64>() / lons.len() as f64;
                    let centroid_lat = lats.iter().sum::<f64>() / lats.len() as f64;
                    Some(Geometry::new(GeoValue::Point(vec![
                        centroid_lon,
                        centroid_lat,
                    ])))
                } else {
                    None
                }
            } else {
                None
            }
        }
    }
}

/// Debug analysis for a specific element
async fn debug_element_analysis(
    element_id: i64,
    data: &OsmData,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    eprintln!("=== DEBUG ANALYSIS FOR ELEMENT {} ===", element_id);

    // Check if element exists
    if let Some(way) = data.ways.get(&element_id) {
        eprintln!("\n=== ELEMENT FOUND ===");
        eprintln!("Type: Way");
        eprintln!("ID: {}", way.id);
        eprintln!("Node count: {}", way.nodes.len());
        eprintln!("Tags:");
        for (key, value) in &way.tags {
            eprintln!("  {}: {}", key, value);
        }

        // Check highway type
        let highway_type = way
            .tags
            .get("highway")
            .map(|s| s.as_str())
            .unwrap_or("none");
        eprintln!("Highway type: {}", highway_type);

        // Check for sidewalk tags
        let has_sidewalk_tags = way.tags.contains_key("sidewalk")
            || way.tags.contains_key("sidewalk:left")
            || way.tags.contains_key("sidewalk:right");
        eprintln!("Has sidewalk tags: {}", has_sidewalk_tags);

        if has_sidewalk_tags {
            eprintln!("  NOTE: This way has sidewalk tags and would be EXCLUDED from pedestrian_completeness analysis");
            if let Some(sidewalk) = way.tags.get("sidewalk") {
                eprintln!("  sidewalk: {}", sidewalk);
            }
            if let Some(left) = way.tags.get("sidewalk:left") {
                eprintln!("  sidewalk:left: {}", left);
            }
            if let Some(right) = way.tags.get("sidewalk:right") {
                eprintln!("  sidewalk:right: {}", right);
            }
        }

        // Now run detailed pedestrian completeness analysis
        if !has_sidewalk_tags {
            debug_pedestrian_analysis(way, data).await?;
        }
    } else if data.nodes.get(&element_id).is_some() {
        eprintln!(
            "Element {} is a node, not a way. Pedestrian analysis only applies to ways.",
            element_id
        );
    } else {
        eprintln!("Element {} not found in loaded data.", element_id);
    }

    Ok(())
}

/// Debug pedestrian completeness analysis for a specific way
async fn debug_pedestrian_analysis(
    way: &oops_core::data::OsmWay,
    data: &OsmData,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    use oops_core::analysis::*;

    eprintln!("\n=== PEDESTRIAN COMPLETENESS DEBUG ===");

    // Create analysis components
    let bbox = BoundingBox::new(-180.0, -90.0, 180.0, 90.0);
    let query = AnalysisQuery::new(bbox);

    // Step 1: Element Selection
    eprintln!("\n--- STEP 1: ELEMENT SELECTION ---");
    let selector = WaySelector::new()
        .highway_types(vec![
            "primary",
            "secondary",
            "tertiary",
            "residential",
            "unclassified",
        ])
        .exclude_with_tag("sidewalk");

    let selected_elements = selector.select_elements(data, &query);
    let target_element = selected_elements.iter().find(|e| e.element_id == way.id);

    if let Some(_element) = target_element {
        eprintln!("✓ Element {} selected for analysis", way.id);
        eprintln!("  Highway type matches filter");
        eprintln!("  No excluding tags found");
    } else {
        eprintln!("✗ Element {} NOT selected for analysis", way.id);
        eprintln!("  This could be due to:");
        eprintln!(
            "  - Highway type not in [primary, secondary, tertiary, residential, unclassified]"
        );
        eprintln!("  - Has 'sidewalk' tag (excluded)");
        eprintln!("  - Not within bounding box");
        return Ok(())
    }

    // Step 2: Spatial Context
    eprintln!("\n--- STEP 2: SPATIAL CONTEXT ---");
    let spatial_context = SpatialContext::with_highway_thresholds(|tags| {
        (tags.get("highway") == Some(&"footway".to_string())
            && tags.get("footway") == Some(&"sidewalk".to_string()))
            || (tags.get("highway") == Some(&"footway".to_string())
                && !tags.contains_key("footway"))
            || (tags.get("highway") == Some(&"path".to_string())
                && tags.get("footway") == Some(&"sidewalk".to_string()))
    });

    // Build spatial index
    let spatial_index = AnalysisSpatialIndex::build(data);
    let enriched_context = spatial_context.enrich_context(target_element.unwrap(), &spatial_index);

    eprintln!(
        "Nearby ways found: {}",
        enriched_context.spatial_context.nearby_ways.len()
    );

    for (i, nearby) in enriched_context
        .spatial_context
        .nearby_ways
        .iter()
        .enumerate()
    {
        eprintln!(
            "  Way {}: ID={}, distance={:.1}m",
            i + 1,
            nearby.way_id,
            nearby.distance
        );
        if let Some(way) = data.ways.get(&nearby.way_id) {
            eprintln!("    Tags: {:?}", way.tags);
        }
    }

    // Step 3: Coverage Calculation
    eprintln!("\n--- STEP 3: COVERAGE CALCULATION ---");

    if !enriched_context.spatial_context.nearby_ways.is_empty() {
        let parallel_sidewalks: Vec<_> = enriched_context
            .spatial_context
            .nearby_ways
            .iter()
            .filter_map(|nearby| data.ways.get(&nearby.way_id))
            .filter_map(|way| {
                oops_core::spatial::way_to_linestring(way, data).map(|linestring| {
                    oops_core::spatial::WayWithLineString {
                        way: way.clone(),
                        linestring,
                    }
                })
            })
            .collect();

        let parallel_sidewalk_refs: Vec<_> = parallel_sidewalks.iter().collect();

        let highway_type = way
            .tags
            .get("highway")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let distance_threshold = match highway_type {
            "residential" | "unclassified" => 20.0,
            "tertiary" => 25.0,
            "secondary" => 30.0,
            "primary" => 35.0,
            _ => 20.0,
        };

        eprintln!(
            "Distance threshold for {}: {}m",
            highway_type,
            distance_threshold
        );
        eprintln!(
            "Parallel sidewalks for coverage calc: {}",
            parallel_sidewalks.len()
        );

        // First run normal coverage to compare
        let normal_coverage = oops_core::spatial::calculate_sidewalk_coverage(
            way,
            &parallel_sidewalk_refs,
            distance_threshold,
            data,
        );

        eprintln!(
            "Normal coverage calculation: {:.1}%",
            normal_coverage * 100.0
        );

        // Now let's debug what might be wrong - detailed sample-by-sample analysis
        eprintln!("\nDebugging sample-by-sample coverage:");
        debug_detailed_sample_coverage(way, &parallel_sidewalk_refs, distance_threshold, data);

        let coverage_ratio = normal_coverage;

        eprintln!(
            "Final Coverage ratio: {:.3} ({:.1}%)",
            coverage_ratio,
            coverage_ratio * 100.0
        );
        eprintln!("Required coverage: 50.0%");
        eprintln!("Meets requirement: {}", coverage_ratio >= 0.5);
    } else {
        eprintln!("No nearby ways found - 0% coverage");
    }

    // Debug: Check if specific ways from user exist in data
    debug_missing_ways(way, data);

    Ok(())
}

/// Debug detailed sample-by-sample coverage analysis
fn debug_detailed_sample_coverage(
    target_way: &oops_core::data::OsmWay,
    parallel_sidewalks: &[&oops_core::spatial::WayWithLineString],
    max_distance: f64,
    data: &OsmData,
) {
    if let Some(target_linestring) = oops_core::spatial::way_to_linestring(target_way, data) {
        // Use same sampling logic as the main function
        let sample_count = (Geodesic.length(&target_linestring) / 10.0)
            .ceil()
            .max(10.0) as usize;
        eprintln!(
            "  Road length: {:.1}m, {} samples (every ~10m)",
            Geodesic.length(&target_linestring),
            sample_count
        );

        let mut covered_samples = 0;

        for i in 0..sample_count {
            let t = i as f64 / (sample_count - 1) as f64;
            let sample_point = target_linestring
                .point_at_ratio_from_start(&Euclidean, t)
                .unwrap();

            // Find closest sidewalk and distance
            let mut closest_distance = f64::MAX;
            let mut closest_sidewalk = None;

            for (idx, sidewalk) in parallel_sidewalks.iter().enumerate() {
                let distance = oops_core::spatial::calculate_point_to_linestring_distance(
                    &sample_point,
                    &sidewalk.linestring,
                );

                if distance < closest_distance {
                    closest_distance = distance;
                    closest_sidewalk = Some((idx + 1, sidewalk.way.id));
                }
            }

            let has_coverage = closest_distance <= max_distance;
            if has_coverage {
                covered_samples += 1;
            }

            let coverage_status = if has_coverage { "✓" } else { "✗" };
            let sidewalk_info = if let Some((idx, id)) = closest_sidewalk {
                format!("SW#{} ({})", idx, id)
            } else {
                "none".to_string()
            };

            eprintln!(
                "  Sample {:2}: {:.6},{:.6} -> {:.1}m to {} {}",
                i + 1,
                sample_point.y(),
                sample_point.x(),
                closest_distance,
                sidewalk_info,
                coverage_status
            );
        }

        let coverage_ratio = covered_samples as f64 / sample_count as f64;
        eprintln!(
            "  Summary: {}/{} samples covered = {:.1}%",
            covered_samples,
            sample_count,
            coverage_ratio * 100.0
        );
    } else {
        eprintln!("  Error: Could not convert target way to linestring");
    }
}

/// Debug missing ways that should provide coverage
fn debug_missing_ways(target_way: &oops_core::data::OsmWay, data: &OsmData) {
    let missing_ways = vec![
        1328892328, 1328892331, 1328892385, 1328892383, 1328892309, 1328892380, 1328892377,
        1328892355, 1328892370,
    ];
    eprintln!("\n--- DEBUGGING MISSING WAYS ---");

    for way_id in missing_ways {
        if let Some(way) = data.ways.get(&way_id) {
            eprintln!("Way {} EXISTS: {:?}", way_id, way.tags);

            // Check if it passes our sidewalk filter
            let tags = &way.tags;
            let highway = tags.get("highway").map(|s| s.as_str());
            let footway = tags.get("footway").map(|s| s.as_str());
            let foot = tags.get("foot").map(|s| s.as_str());
            let bicycle = tags.get("bicycle").map(|s| s.as_str());

            let passes_filter = match highway {
                Some("footway") => {
                    footway == Some("sidewalk") || footway == Some("crossing") || footway.is_none()
                }
                Some("path") => {
                    footway == Some("sidewalk")
                        || (bicycle == Some("designated") && foot != Some("no"))
                }
                Some("cycleway") => {
                    foot == Some("yes")
                        || foot == Some("designated")
                        || foot == Some("permissive")
                        || foot.is_none()
                }
                _ => false,
            };

            eprintln!("  Passes sidewalk filter: {}", passes_filter);

            // Check distance to target way
            if let Some(target_ls) = oops_core::spatial::way_to_linestring(target_way, data) {
                if let Some(way_ls) = oops_core::spatial::way_to_linestring(way, data) {
                    let distance = oops_core::spatial::calculate_linestring_to_linestring_distance(
                        &target_ls, &way_ls,
                    );
                    eprintln!("  Distance to target way: {:.1}m", distance);
                    eprintln!("  Within 25m threshold: {}", distance <= 25.0);
                } else {
                    eprintln!("  Could not convert to linestring (missing nodes?)");
                }
            }
        } else {
            eprintln!("Way {} NOT FOUND in loaded data", way_id);
        }
    }
}