use clap::{Parser, Subcommand};
use oops_analyzers::{MissingMaxspeedAnalyzer, MissingStreetNamesAnalyzer};
use oops_core::{AnalysisQuery, AnalysisResult, AnalyzerRegistry, BoundingBox};
use oops_ingest::{PbfFilter, PbfReader};
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
    let mut registry = AnalyzerRegistry::new();
    registry.register(MissingStreetNamesAnalyzer::new());
    registry.register(MissingMaxspeedAnalyzer::new());

    // Run analysis
    let bbox = query_bbox.unwrap_or_else(|| BoundingBox::new(-180.0, -90.0, 180.0, 90.0));
    let query = AnalysisQuery::new(bbox);

    let results = if let Some(analyzer_name) = analyzer {
        // Run specific analyzer
        vec![registry.analyze(&analyzer_name, &query, &data)?]
    } else {
        // Run all analyzers
        registry.analyze_all(&query, &data).into_values().collect()
    };

    // Output results
    output_results(results, format)?;

    Ok(())
}

fn list_analyzers() {
    let mut registry = AnalyzerRegistry::new();
    registry.register(MissingStreetNamesAnalyzer::new());
    registry.register(MissingMaxspeedAnalyzer::new());

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
    results: Vec<Vec<AnalysisResult>>,
    format: OutputFormat,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    match format {
        OutputFormat::Json => {
            let flattened: Vec<&AnalysisResult> = results.iter().flatten().collect();
            println!("{}", serde_json::to_string_pretty(&flattened)?);
        }
        OutputFormat::Summary => {
            let total_results: usize = results.iter().map(|r| r.len()).sum();
            let total_findings: usize = results.iter().flatten().map(|r| r.findings.len()).sum();

            println!("Analysis Summary:");
            println!("  Elements analyzed: {}", total_results);
            println!("  Issues found: {}", total_findings);

            if total_findings > 0 {
                println!("\nIssues:");
                for result in results.iter().flatten() {
                    if !result.findings.is_empty() {
                        println!(
                            "  Element {}: {} issues",
                            result.element_id,
                            result.findings.len()
                        );
                        for finding in &result.findings {
                            println!(
                                "    - {}",
                                finding.message.as_ref().unwrap_or(&"No message".into())
                            );
                        }
                    }
                }
            }
        }
        OutputFormat::Csv => {
            println!("element_id,analyzer,severity,message,context");
            for result in results.iter().flatten() {
                let analyzer = "missing_street_names"; // TODO: get from result when available
                for finding in &result.findings {
                    let context = serde_json::to_string(&finding.context).unwrap_or_default();
                    println!(
                        "{},{},{:?},{},\"{}\"",
                        result.element_id,
                        analyzer,
                        finding.severity,
                        finding.message.as_ref().unwrap_or(&"No message".into()),
                        context
                    );
                }
            }
        }
    }

    Ok(())
}
