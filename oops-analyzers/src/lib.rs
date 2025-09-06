// OOPS Analyzers - Built-in analyzers

pub mod integration_tests;
pub mod missing_maxspeed;
pub mod pedestrian_completeness;
pub mod street_names;

pub use missing_maxspeed::*;
pub use pedestrian_completeness::*;
pub use street_names::*;

use oops_core::Analyzer;

/// Returns a vector containing an instance of each available analyzer.
pub fn get_all_analyzers() -> Vec<Box<dyn Analyzer>> {
    vec![
        Box::new(MissingMaxspeedAnalyzer::new()),
        Box::new(MissingStreetNamesAnalyzer::new()),
        Box::new(PedestrianCompletenessAnalyzer::new()),
    ]
}

