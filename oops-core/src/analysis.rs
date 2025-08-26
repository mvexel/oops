use crate::data::{BoundingBox, OsmId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElementType {
    Node,
    Way,
    Relation,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProblemCategory {
    Missing,
    Outdated,
    Invalid,
    Inconsistent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub category: ProblemCategory,
    pub severity: Severity,
    pub message: String,
    pub recommendation: Option<String>,
    pub context: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub element_id: OsmId,
    pub element_type: ElementType,
    pub findings: Vec<Finding>,
    pub overall_severity: Severity,
    pub confidence: f64,
}

impl AnalysisResult {
    pub fn new(element_id: OsmId, element_type: ElementType) -> Self {
        Self {
            element_id,
            element_type,
            findings: Vec::new(),
            overall_severity: Severity::Info,
            confidence: 1.0,
        }
    }

    pub fn add_finding(&mut self, finding: Finding) {
        if finding.severity > self.overall_severity {
            self.overall_severity = finding.severity.clone();
        }
        self.findings.push(finding);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ElementRef {
    Node(OsmId),
    Way(OsmId),
    Relation(OsmId),
    Intersection(OsmId), // Special case: node with degree 3+
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisQuery {
    pub area: BoundingBox,
    pub elements: Vec<ElementRef>,
    pub context: AnalysisContext,
}

impl AnalysisQuery {
    pub fn new(area: BoundingBox) -> Self {
        Self {
            area,
            elements: Vec::new(),
            context: AnalysisContext::default(),
        }
    }

    pub fn with_element(mut self, element: ElementRef) -> Self {
        self.elements.push(element);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisContext {
    pub buffer_distance: f64,
    pub parameters: HashMap<String, String>,
}

impl Default for AnalysisContext {
    fn default() -> Self {
        Self {
            buffer_distance: 50.0, // Default 50m buffer
            parameters: HashMap::new(),
        }
    }
}

pub trait Analyzer: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn description(&self) -> &str;
    fn analyze(&self, query: &AnalysisQuery, data: &crate::data::OsmData) -> Vec<AnalysisResult>;
}

/// Registry for managing analyzers
#[derive(Default)]
pub struct AnalyzerRegistry {
    analyzers: HashMap<String, Box<dyn Analyzer>>,
}

impl std::fmt::Debug for AnalyzerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalyzerRegistry")
            .field(
                "analyzers",
                &format!("{} registered analyzers", self.analyzers.len()),
            )
            .finish()
    }
}

impl AnalyzerRegistry {
    pub fn new() -> Self {
        Self {
            analyzers: HashMap::new(),
        }
    }

    /// Register an analyzer by name
    pub fn register<A: Analyzer + 'static>(&mut self, analyzer: A) {
        let name = analyzer.name().to_string();
        self.analyzers.insert(name, Box::new(analyzer));
    }

    /// Get analyzer by name
    pub fn get(&self, name: &str) -> Option<&dyn Analyzer> {
        self.analyzers.get(name).map(|a| a.as_ref())
    }

    /// List all registered analyzers
    pub fn list(&self) -> Vec<&str> {
        self.analyzers.keys().map(|s| s.as_str()).collect()
    }

    /// Run analysis with a specific analyzer
    pub fn analyze(
        &self,
        analyzer_name: &str,
        query: &AnalysisQuery,
        data: &crate::data::OsmData,
    ) -> Result<Vec<AnalysisResult>, String> {
        match self.get(analyzer_name) {
            Some(analyzer) => Ok(analyzer.analyze(query, data)),
            None => Err(format!(
                "Analyzer '{}' not found. Available: {}",
                analyzer_name,
                self.list().join(", ")
            )),
        }
    }

    /// Run analysis with all registered analyzers
    pub fn analyze_all(
        &self,
        query: &AnalysisQuery,
        data: &crate::data::OsmData,
    ) -> HashMap<String, Vec<AnalysisResult>> {
        let mut results = HashMap::new();

        for (name, analyzer) in &self.analyzers {
            let analysis_results = analyzer.analyze(query, data);
            results.insert(name.clone(), analysis_results);
        }

        results
    }

    /// Get analyzer metadata
    pub fn get_info(&self, name: &str) -> Option<AnalyzerInfo> {
        self.get(name).map(|analyzer| AnalyzerInfo {
            name: analyzer.name().to_string(),
            version: analyzer.version().to_string(),
            description: analyzer.description().to_string(),
        })
    }

    /// Get all analyzer metadata
    pub fn get_all_info(&self) -> Vec<AnalyzerInfo> {
        self.analyzers
            .values()
            .map(|analyzer| AnalyzerInfo {
                name: analyzer.name().to_string(),
                version: analyzer.version().to_string(),
                description: analyzer.description().to_string(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerInfo {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzerConfig {
    pub name: String,
    pub parameters: HashMap<String, String>,
}

impl AnalyzerConfig {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            parameters: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.parameters.insert(key.to_string(), value.to_string());
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.parameters.get(key)
    }
}
