use clap::Parser;
use geojson::{Feature, FeatureCollection, Geometry, Value};
use oops_analyzers::IntersectionAnalyzer;
use oops_ingest::pbf::PbfReader;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the OSM PBF file to analyze
    #[arg(short, long)]
    pbf_file: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    eprintln!("🚀 OOPS Intersection Analyzer v2");
    eprintln!("==================================================");

    // Load OSM data
    eprintln!("\n📊 Loading OSM data from {:?}...", args.pbf_file);
    let reader = PbfReader::new();
    let data = reader.read_file(&args.pbf_file, None).await.unwrap();
    eprintln!("   Loaded {} nodes, {} ways, {} relations", data.nodes.len(), data.ways.len(), data.relations.len());

    // Run intersection analysis
    eprintln!("\n🔍 Analyzing intersections...");
    let analyzer = IntersectionAnalyzer::new(30.0);
    let intersections = analyzer.analyze(&data);
    eprintln!("   Found {} logical intersections", intersections.len());

    // Create GeoJSON output
    let features: Vec<Feature> = intersections
        .into_iter()
        .map(|intersection| {
            let mut feature = Feature {
                geometry: Some(Geometry::new(Value::Point(vec![intersection.coordinates.x(), intersection.coordinates.y()]))),
                ..Default::default()
            };
            feature.set_property("representative_node", intersection.representative_node);
            feature.set_property("node_cluster", intersection.node_cluster.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","));
            feature.set_property("degree", intersection.degree);
            feature.set_property("intersection_type", intersection.intersection_type);
            feature
        })
        .collect();

    let feature_collection = FeatureCollection {
        bbox: None,
        features,
        foreign_members: None,
    };

    let geojson_string = serde_json::to_string_pretty(&feature_collection).unwrap();
    println!("{}", geojson_string);
}

