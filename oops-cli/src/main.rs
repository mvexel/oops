use clap::{Parser, Subcommand};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, Value as GeoValue};
use oops_analyzers::get_all_analyzers;
use oops_core::{
    AnalysisQuery, AnalysisResult, AnalyzerRegistry, BoundingBox, ElementType, OsmData,
};
use oops_ingest::{PbfFilter, PbfReader};
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

        /// Analyzer to run (default: all)
        #[arg(short, long)]
        analyzer: Option<String>,

        /// Output format
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,
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
            analyzer,
            format,
            verbose,
        } => {
            analyze_command(file, bbox, analyzer, format, verbose).await?;
        }
        Commands::List => {
            list_analyzers();
        }
    }

    Ok(())
}

/// Create a new analyzer registry and populate it with all available analyzers.
fn create_and_populate_registry() -> AnalyzerRegistry {
    let mut registry = AnalyzerRegistry::new();
    for analyzer in get_all_analyzers() {
        registry.register(analyzer);
    }
    registry
}

async fn analyze_command(
    file: Option<PathBuf>,
    bbox: Option<String>,
    analyzer: Option<String>,
    format: OutputFormat,
    verbose: bool,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    if verbose {
        println!("Loading OSM data...");
    }

    // Parse bounding box if provided
    let query_bbox = if let Some(bbox_str) = bbox {
        Some(parse_bbox(&bbox_str)?)
    } else {
        None
    };

    // Load data from file or error if no input
    let data = if let Some(pbf_path) = file {
        if verbose {
            println!("Reading PBF file: {:?}", pbf_path);
        }
        let reader = PbfReader::new();
        let filter = if let Some(bbox) = &query_bbox {
            Some(PbfFilter::new().in_bbox(bbox))
        } else {
            None
        };
        reader.read_file(&pbf_path, filter).await?
    } else {
        return Err("Either --file or --bbox with Overpass API must be provided".into());
    };

    if verbose {
        println!(
            "Loaded {} nodes and {} ways",
            data.nodes.len(),
            data.ways.len()
        );
    }

    // Set up analyzer registry
    let registry = create_and_populate_registry();

    // Run analysis
    let bbox = query_bbox.unwrap_or_else(|| BoundingBox::new(-180.0, -90.0, 180.0, 90.0));
    let query = AnalysisQuery::new(bbox);

    let results: Vec<(String, Vec<AnalysisResult>)> = if let Some(analyzer_name) = analyzer {
        // Run specific analyzer
        vec![(
            analyzer_name.clone(),
            registry.analyze(&analyzer_name, &query, &data)?,
        )]
    } else {
        // Run all analyzers
        registry.analyze_all(&query, &data).into_iter().collect()
    };

    // Output results
    output_results(results, format, &data)?;

    Ok(())
}

fn list_analyzers() {
    let registry = create_and_populate_registry();

    println!("Available analyzers:");
    for info in registry.get_all_info() {
        println!("  {} (v{}) - {}", info.name, info.version, info.description);
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

            println!("Analysis Summary:");
            println!("  Issues found: {}", total_findings);

            if total_findings > 0 {
                println!("\nIssues by Analyzer:");
                for (analyzer_name, analyzer_results) in &results {
                    if !analyzer_results.is_empty() {
                        println!("  - {}: {} issues", analyzer_name, analyzer_results.len());
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
                            if let Some(recommendation) = &finding.recommendation {
                                properties.insert(
                                    "recommendation".to_string(),
                                    serde_json::Value::String(recommendation.clone()),
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
        ElementType::Node => {
            if let Some(node) = osm_data.get_node(element_id) {
                Some(Geometry::new(GeoValue::Point(vec![node.lon, node.lat])))
            } else {
                None
            }
        }
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
