use crate::data::{BoundingBox, OsmData, OsmId};
use crate::spatial::calculate_linestring_to_linestring_distance;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ElementType {
    Node,
    Way,
    Relation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub category: ProblemCategory,
    pub severity: Severity,
    pub message: Option<String>,
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
            self.overall_severity = finding.severity;
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

// ============================================================================
// ELEMENT SELECTION
// ============================================================================

/// Selected OSM element ready for analysis
#[derive(Debug, Clone)]
pub struct SelectedElement {
    pub element_id: i64,
    pub element_type: ElementType,
    pub tags: HashMap<String, String>,
    pub geometry: ElementGeometry,
}

/// Geometry information for selected elements
#[derive(Debug, Clone)]
pub enum ElementGeometry {
    Point { lat: f64, lon: f64 },
    LineString { nodes: Vec<(f64, f64)> },
}

/// Selects which OSM elements should be analyzed
pub trait ElementSelector: Send + Sync {
    fn select_elements(&self, data: &OsmData, query: &AnalysisQuery) -> Vec<SelectedElement>;
}

/// Selects ways based on configurable criteria
pub struct WaySelector {
    highway_types: Vec<String>,
    exclude_with_tag: Vec<String>,
}

impl WaySelector {
    pub fn new() -> Self {
        Self {
            highway_types: Vec::new(),
            exclude_with_tag: Vec::new(),
        }
    }

    pub fn highway_types(mut self, types: Vec<&str>) -> Self {
        self.highway_types = types.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn exclude_with_tag(mut self, key: &str) -> Self {
        self.exclude_with_tag.push(key.to_string());
        self
    }
}

impl ElementSelector for WaySelector {
    fn select_elements(&self, data: &OsmData, query: &AnalysisQuery) -> Vec<SelectedElement> {
        data.ways
            .values()
            .filter(|way| {
                // Highway type check
                if !self.highway_types.is_empty() {
                    match way.tags.get("highway") {
                        Some(highway) if self.highway_types.contains(highway) => {}
                        _ => return false,
                    }
                }

                // Exclude tags check
                for key in &self.exclude_with_tag {
                    if way.tags.contains_key(key) {
                        return false;
                    }
                }

                true
            })
            .filter_map(|way| {
                // Spatial filter - way in query area
                let has_nodes_in_area = way.nodes.iter().any(|&node_id| {
                    if let Some(node) = data.get_node(node_id) {
                        query.area.contains_point(node.lon, node.lat)
                    } else {
                        false
                    }
                });

                if !has_nodes_in_area {
                    return None;
                }

                // Check if way extends beyond PBF boundary - if so, exclude it
                if let Some(boundary) = &data.boundary {
                    let all_nodes_within_boundary = way.nodes.iter().all(|&node_id| {
                        if let Some(node) = data.get_node(node_id) {
                            boundary.contains_point(node.lon, node.lat)
                        } else {
                            false // Missing node - exclude way
                        }
                    });

                    if !all_nodes_within_boundary {
                        return None; // Exclude way that extends beyond boundary
                    }
                }

                Some(way.clone())
            })
            .filter_map(|way| {
                // Convert to SelectedElement
                let nodes: Vec<(f64, f64)> = way
                    .nodes
                    .iter()
                    .filter_map(|&node_id| data.get_node(node_id).map(|node| (node.lon, node.lat)))
                    .collect();

                if nodes.is_empty() {
                    return None;
                }

                Some(SelectedElement {
                    element_id: way.id,
                    element_type: ElementType::Way,
                    tags: way.tags,
                    geometry: ElementGeometry::LineString { nodes },
                })
            })
            .collect()
    }
}

/// Selects intersection nodes
pub struct IntersectionSelector {
    pub cluster_distance: f64,
}

impl IntersectionSelector {
    pub fn new(cluster_distance: f64) -> Self {
        Self { cluster_distance }
    }
}

impl ElementSelector for IntersectionSelector {
    fn select_elements(&self, data: &OsmData, query: &AnalysisQuery) -> Vec<SelectedElement> {
        use crate::intersections::IntersectionIndex;
        use geo::Rect;

        let bbox = Rect::new(
            geo::Coord {
                x: query.area.min_lon,
                y: query.area.min_lat,
            },
            geo::Coord {
                x: query.area.max_lon,
                y: query.area.max_lat,
            },
        );

        let intersection_index = IntersectionIndex::build(data, Some(bbox));
        let (road_graph, node_map) = crate::graph::build_road_network(data);

        let logical_intersections = intersection_index.get_logical_intersections(
            self.cluster_distance,
            Some((&road_graph, &node_map)),
        );

        logical_intersections
            .into_iter()
            .filter_map(|li| {
                data.nodes.get(&li.representative_node).map(|node| {
                    let mut tags = node.tags.clone();
                    tags.insert(
                        "cluster_nodes".to_string(),
                        li.node_cluster
                            .iter()
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                    SelectedElement {
                        element_id: li.representative_node,
                        element_type: ElementType::Node,
                        tags,
                        geometry: ElementGeometry::Point {
                            lat: node.lat,
                            lon: node.lon,
                        },
                    }
                })
            })
            .collect()
    }
}

impl Default for WaySelector {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CONTEXT ENRICHMENT
// ============================================================================

/// Rich context information for analysis
#[derive(Debug, Clone)]
pub struct EnrichedContext {
    pub element: SelectedElement,
    pub spatial_context: SpatialContextData,
    pub metadata: HashMap<String, String>,
}

/// Spatial relationships and nearby elements
#[derive(Debug, Clone, Default)]
pub struct SpatialContextData {
    pub nearby_ways: Vec<NearbyWay>,
}

/// Information about a nearby way
#[derive(Debug, Clone)]
pub struct NearbyWay {
    pub way_id: i64,
    pub distance: f64,
    pub tags: HashMap<String, String>,
    pub angle_difference: Option<f64>,
}

/// Enriches selected elements with contextual information
pub trait ContextProvider: Send + Sync {
    fn enrich_context(
        &self,
        element: &SelectedElement,
        spatial_index: &AnalysisSpatialIndex,
    ) -> EnrichedContext;
}

/// No-op context provider for simple analyzers
pub struct NoContext;

impl ContextProvider for NoContext {
    fn enrich_context(
        &self,
        element: &SelectedElement,
        _spatial_index: &AnalysisSpatialIndex,
    ) -> EnrichedContext {
        EnrichedContext {
            element: element.clone(),
            spatial_context: SpatialContextData::default(),
            metadata: HashMap::new(),
        }
    }
}

/// Generic spatial context provider for finding nearby ways with custom filters
pub struct SpatialContext {
    pub max_distance: f64,
    pub max_angle_diff: f64,
    pub highway_distance_map: HashMap<String, f64>,
    pub way_filter: Box<dyn Fn(&HashMap<String, String>) -> bool + Send + Sync>,
}

impl SpatialContext {
    pub fn new<F>(max_distance: f64, max_angle_diff: f64, way_filter: F) -> Self
    where
        F: Fn(&HashMap<String, String>) -> bool + Send + Sync + 'static,
    {
        Self {
            max_distance,
            max_angle_diff,
            highway_distance_map: HashMap::new(),
            way_filter: Box::new(way_filter),
        }
    }

    /// Create with highway-specific distance thresholds and custom filter
    pub fn with_highway_thresholds<F>(way_filter: F) -> Self
    where
        F: Fn(&HashMap<String, String>) -> bool + Send + Sync + 'static,
    {
        let mut highway_map = HashMap::new();
        highway_map.insert("residential".to_string(), 20.0);
        highway_map.insert("unclassified".to_string(), 20.0);
        highway_map.insert("tertiary".to_string(), 25.0);
        highway_map.insert("secondary".to_string(), 30.0);
        highway_map.insert("primary".to_string(), 35.0);

        Self {
            max_distance: 35.0,   // Maximum of all thresholds
            max_angle_diff: 30.0, // Legacy default
            highway_distance_map: highway_map,
            way_filter: Box::new(way_filter),
        }
    }

    /// Get the appropriate distance threshold for a highway type
    pub fn get_distance_for_highway(&self, highway_type: &str) -> f64 {
        self.highway_distance_map
            .get(highway_type)
            .copied()
            .unwrap_or(self.max_distance)
    }
}

impl ContextProvider for SpatialContext {
    fn enrich_context(
        &self,
        element: &SelectedElement,
        spatial_index: &AnalysisSpatialIndex,
    ) -> EnrichedContext {
        // Get highway-specific distance threshold
        let highway_type = element
            .tags
            .get("highway")
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let distance_threshold = self.get_distance_for_highway(highway_type);

        let nearby_ways = spatial_index.find_nearby_ways_by_tags(
            element,
            distance_threshold,
            &*self.way_filter, // Use the configurable filter instead of hard-coded logic
        );

        EnrichedContext {
            element: element.clone(),
            spatial_context: SpatialContextData { nearby_ways },
            metadata: HashMap::new(),
        }
    }
}

/// Context provider that clusters nearby intersection nodes into logical intersections
pub struct IntersectionContextProvider;

impl IntersectionContextProvider {
    pub fn new() -> Self {
        Self
    }
}

impl ContextProvider for IntersectionContextProvider {
    fn enrich_context(
        &self,
        element: &SelectedElement,
        spatial_index: &AnalysisSpatialIndex,
    ) -> EnrichedContext {
        let mut metadata = HashMap::new();
        let osm_data = spatial_index.osm_data();

        let cluster_nodes: Vec<i64> = element
            .tags
            .get("cluster_nodes")
            .map(|s| s.split(',').filter_map(|n| n.parse().ok()).collect())
            .unwrap_or_else(|| vec![element.element_id]);

        // Use efficient node-to-ways index instead of iterating through all ways
        let node_to_ways = crate::intersections::build_node_to_ways_index(osm_data);
        let mut connected_ways = std::collections::HashSet::new();
        
        for &node_id in &cluster_nodes {
            if let Some(way_ids) = node_to_ways.get(&node_id) {
                for &way_id in way_ids {
                    connected_ways.insert(way_id);
                }
            }
        }

        let degree = connected_ways.len();
        let intersection_type = match degree {
            3 => "3-way".to_string(),
            4 => "4-way".to_string(),
            d if d >= 5 => "complex".to_string(),
            _ => "simple".to_string(),
        };

        let roundabout_ways: Vec<i64> = connected_ways.iter().filter_map(|&way_id| {
            osm_data.ways.get(&way_id).and_then(|w| {
                if w.tags.get("junction") == Some(&"roundabout".to_string()) {
                    Some(way_id)
                } else {
                    None
                }
            })
        }).collect();

        let has_roundabout = !roundabout_ways.is_empty();
        
        let final_type = if has_roundabout {
            "roundabout".to_string()
        } else if cluster_nodes.len() > 1 {
            "complex".to_string()
        } else {
            intersection_type
        };

        metadata.insert("degree".to_string(), degree.to_string());
        metadata.insert("intersection_type".to_string(), final_type);
        metadata.insert("cluster_size".to_string(), cluster_nodes.len().to_string());
        metadata.insert(
            "node_cluster".to_string(),
            cluster_nodes.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        );
        metadata.insert(
            "connected_ways".to_string(),
            connected_ways.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
        );
        
        if has_roundabout {
            metadata.insert(
                "roundabout_ways".to_string(),
                roundabout_ways.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
            );
        }

        // Count one-way roads and their direction relative to intersection nodes
        let mut oneway_departing = 0;
        let mut oneway_arriving = 0;
        let mut oneway_ways_departing = Vec::new();
        let mut oneway_ways_arriving = Vec::new();
        
        for &way_id in &connected_ways {
            if let Some(way) = osm_data.ways.get(&way_id) {
                let is_oneway = way.tags.get("oneway").map_or(false, |v| v == "yes");
                let is_reverse_oneway = way.tags.get("oneway").map_or(false, |v| v == "-1");
                
                if is_oneway || is_reverse_oneway {
                    // Determine direction relative to intersection nodes
                    // Check if any cluster node is at the start or end of the way
                    let first_node = way.nodes.first().copied();
                    let last_node = way.nodes.last().copied();
                    
                    let cluster_at_start = cluster_nodes.iter().any(|&n| Some(n) == first_node);
                    let cluster_at_end = cluster_nodes.iter().any(|&n| Some(n) == last_node);
                    
                    if is_oneway {
                        if cluster_at_start {
                            oneway_departing += 1;
                            oneway_ways_departing.push(way_id);
                        }
                        if cluster_at_end {
                            oneway_arriving += 1;
                            oneway_ways_arriving.push(way_id);
                        }
                    } else if is_reverse_oneway {
                        // Reverse oneway: traffic flows opposite to way direction
                        if cluster_at_start {
                            oneway_arriving += 1;
                            oneway_ways_arriving.push(way_id);
                        }
                        if cluster_at_end {
                            oneway_departing += 1;
                            oneway_ways_departing.push(way_id);
                        }
                    }
                }
            }
        }
        
        metadata.insert("oneway_departing_count".to_string(), oneway_departing.to_string());
        metadata.insert("oneway_arriving_count".to_string(), oneway_arriving.to_string());
        
        if !oneway_ways_departing.is_empty() {
            metadata.insert(
                "oneway_ways_departing".to_string(),
                oneway_ways_departing.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
            );
        }
        
        if !oneway_ways_arriving.is_empty() {
            metadata.insert(
                "oneway_ways_arriving".to_string(),
                oneway_ways_arriving.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","),
            );
        }

        EnrichedContext {
            element: element.clone(),
            spatial_context: SpatialContextData::default(),
            metadata,
        }
    }
}

// ============================================================================
// PROBLEM DETECTION
// ============================================================================

/// Detects problems in enriched analysis context
pub trait ProblemDetector: Send + Sync {
    fn detect_problems(&self, context: &EnrichedContext, data: &OsmData) -> Vec<Finding>;
}

/// Detects missing tags - perfect for simple analyzers like missing_maxspeed
pub struct MissingTagDetector {
    required_tag: String,
    severity: Severity,
    message_template: String,
}

impl MissingTagDetector {
    pub fn new(tag: &str) -> Self {
        Self {
            required_tag: tag.to_string(),
            severity: Severity::Medium,
            message_template: "Element is missing the '{}' tag".to_string(),
        }
    }

    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn with_message_template(mut self, template: &str) -> Self {
        self.message_template = template.to_string();
        self
    }
}

impl ProblemDetector for MissingTagDetector {
    fn detect_problems(&self, context: &EnrichedContext, _data: &OsmData) -> Vec<Finding> {
        // Check if the required tag is missing
        if context.element.tags.contains_key(&self.required_tag) {
            return Vec::new();
        }

        let mut finding_context = HashMap::new();
        finding_context.insert(
            "element_id".to_string(),
            context.element.element_id.to_string(),
        );
        finding_context.insert("missing_tag".to_string(), self.required_tag.clone());

        // Add existing tags to context
        for (key, value) in &context.element.tags {
            finding_context.insert(format!("tag_{}", key), value.clone());
        }

        let message = self.message_template.replace("{}", &self.required_tag);

        vec![Finding {
            category: ProblemCategory::Missing,
            severity: self.severity,
            message: Some(message),
            context: finding_context,
        }]
    }
}

// ============================================================================
// SPATIAL INDEXING
// ============================================================================

/// Spatial query parameters
pub struct SpatialQuery {
    pub max_distance: f64,
    pub max_angle_diff: Option<f64>,
}

/// Analysis-specific spatial index wrapper
pub struct AnalysisSpatialIndex {
    osm_data: OsmData,
    spatial_index: crate::spatial_index::SpatialIndex,
}

impl AnalysisSpatialIndex {
    /// Build spatial index from OSM data
    pub fn build(data: &OsmData) -> Self {
        Self {
            spatial_index: crate::spatial_index::SpatialIndex::build(data),
            osm_data: data.clone(),
        }
    }

    /// Access to underlying OSM data
    pub fn osm_data(&self) -> &OsmData {
        &self.osm_data
    }

    /// Access to high-performance spatial index
    pub fn spatial_index(&self) -> &crate::spatial_index::SpatialIndex {
        &self.spatial_index
    }

    /// Find potential intersection candidates using spatial index - O(log n)
    pub fn find_intersecting_way_candidates(
        &self,
        way: &crate::OsmWay,
        max_distance: f64,
    ) -> Vec<i64> {
        self.spatial_index
            .find_intersecting_ways(way, &self.osm_data, max_distance)
    }

    /// Find nearby ways using generic tag-based filter - O(log n) instead of O(n)  
    pub fn find_nearby_ways_by_tags<F>(
        &self,
        element: &SelectedElement,
        max_distance: f64,
        tag_filter: F,
    ) -> Vec<NearbyWay>
    where
        F: Fn(&std::collections::HashMap<String, String>) -> bool,
    {
        // Get the target way from OSM data
        let Some(target_way) = self.osm_data.ways.get(&element.element_id) else {
            return Vec::new();
        };

        // Use high-performance spatial index for O(log n) query
        let nearby_ways = self.spatial_index.find_nearby_ways_by_tags(
            target_way,
            &self.osm_data,
            max_distance,
            tag_filter,
        );

        // Convert to NearbyWay format
        nearby_ways
            .into_iter()
            .map(|way_with_linestring| {
                let way = &way_with_linestring.way;
                let distance = if let Some(target_geometry) =
                    crate::spatial::way_to_linestring(target_way, &self.osm_data)
                {
                    calculate_linestring_to_linestring_distance(
                        &target_geometry,
                        &way_with_linestring.linestring,
                    )
                } else {
                    0.0
                };

                NearbyWay {
                    way_id: way.id,
                    distance,
                    tags: way.tags.clone(),
                    angle_difference: Some(0.0), // Could calculate if needed
                }
            })
            .collect()
    }
}

// ============================================================================
// ANALYSIS ENGINE
// ============================================================================

/// High-performance analysis engine with parallel processing and caching
pub struct AnalysisEngine {
    spatial_index: Option<AnalysisSpatialIndex>,
}

impl AnalysisEngine {
    pub fn new() -> Self {
        Self {
            spatial_index: None,
        }
    }

    /// Run multiple analyzers in parallel on the same dataset
    pub fn analyze(
        &mut self,
        analyzers: &[Analyzer],
        query: &AnalysisQuery,
        data: &OsmData,
    ) -> Vec<AnalysisResult> {
        use rayon::prelude::*;

        // Build shared spatial index once for all analyzers
        if self.spatial_index.is_none() {
            self.spatial_index = Some(AnalysisSpatialIndex::build(data));
        }

        let spatial_index = self.spatial_index.as_ref().unwrap();

        // Run analyzers in parallel for massive speedup
        analyzers
            .par_iter()
            .flat_map(|analyzer| self.run_analyzer(analyzer, query, spatial_index))
            .collect()
    }

    /// Run a single analyzer with full optimization
    pub fn analyze_single(
        &mut self,
        analyzer: &Analyzer,
        query: &AnalysisQuery,
        data: &OsmData,
    ) -> Vec<AnalysisResult> {
        if self.spatial_index.is_none() {
            self.spatial_index = Some(AnalysisSpatialIndex::build(data));
        }

        let spatial_index = self.spatial_index.as_ref().unwrap();
        self.run_analyzer(analyzer, query, spatial_index)
    }

    fn run_analyzer(
        &self,
        analyzer: &Analyzer,
        query: &AnalysisQuery,
        spatial_index: &AnalysisSpatialIndex,
    ) -> Vec<AnalysisResult> {
        use rayon::prelude::*;

        // Step 1: Select elements
        let selected_elements = analyzer
            .selector
            .select_elements(spatial_index.osm_data(), query);
        let data = spatial_index.osm_data();

        // Step 2: Process elements in parallel
        let results: Vec<_> = selected_elements
            .par_iter()
            .filter_map(|element| {
                // Step 3: Enrich with context
                let context = analyzer
                    .context_provider
                    .enrich_context(element, spatial_index);

                // Step 4: Detect problems
                let findings = analyzer.detector.detect_problems(&context, data);

                // Step 5: Create result if there are findings
                if !findings.is_empty() {
                    let mut result =
                        AnalysisResult::new(element.element_id, element.element_type.clone());
                    for finding in findings {
                        result.add_finding(finding);
                    }
                    Some(result)
                } else {
                    None
                }
            })
            .collect();

        // Sort results by element ID for consistent ordering
        let mut sorted_results = results;
        sorted_results.sort_by_key(|r| r.element_id);
        sorted_results
    }

    /// Clear caches and force rebuild on next analysis
    pub fn clear_cache(&mut self) {
        if let Some(spatial_index) = &self.spatial_index {
            spatial_index.spatial_index().clear_cache();
        }
        self.spatial_index = None;
    }

    /// Get performance statistics
    pub fn get_stats(&self) -> Option<crate::spatial_index::SpatialIndexStats> {
        self.spatial_index
            .as_ref()
            .map(|si| si.spatial_index().stats())
    }
}

impl Default for AnalysisEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// COMPOSABLE ANALYZER
// ============================================================================

/// Composable analyzer built from pipeline components
pub struct Analyzer {
    pub name: String,
    pub version: String,
    pub description: String,
    pub selector: Box<dyn ElementSelector>,
    pub context_provider: Box<dyn ContextProvider>,
    pub detector: Box<dyn ProblemDetector>,
}

impl Analyzer {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        selector: Box<dyn ElementSelector>,
        context_provider: Box<dyn ContextProvider>,
        detector: Box<dyn ProblemDetector>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: description.into(),
            selector,
            context_provider,
            detector,
        }
    }

    /// Get analyzer info
    pub fn info(&self) -> AnalyzerInfo {
        AnalyzerInfo {
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
        }
    }
}

// ============================================================================
// ANALYZER REGISTRY
// ============================================================================

/// Registry for managing composable analyzers
#[derive(Default)]
pub struct AnalyzerRegistry {
    analyzers: HashMap<String, Analyzer>,
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
    pub fn register(&mut self, analyzer: Analyzer) {
        let name = analyzer.name.clone();
        self.analyzers.insert(name, analyzer);
    }

    /// Get analyzer by name
    pub fn get(&self, name: &str) -> Option<&Analyzer> {
        self.analyzers.get(name)
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
        data: &OsmData,
    ) -> Result<Vec<AnalysisResult>, String> {
        match self.get(analyzer_name) {
            Some(analyzer) => {
                let mut engine = AnalysisEngine::new();
                Ok(engine.analyze_single(analyzer, query, data))
            }
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
        data: &OsmData,
    ) -> HashMap<String, Vec<AnalysisResult>> {
        let mut results = HashMap::new();
        let mut engine = AnalysisEngine::new();

        for (name, analyzer) in &self.analyzers {
            let analysis_results = engine.analyze_single(analyzer, query, data);
            results.insert(name.clone(), analysis_results);
        }

        results
    }

    /// Get analyzer metadata
    pub fn get_info(&self, name: &str) -> Option<AnalyzerInfo> {
        self.get(name).map(|analyzer| analyzer.info())
    }

    /// Get all analyzer metadata
    pub fn get_all_info(&self) -> Vec<AnalyzerInfo> {
        self.analyzers
            .values()
            .map(|analyzer| analyzer.info())
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
            name: name.into(),
            parameters: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.parameters.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.parameters.get(key)
    }
}
