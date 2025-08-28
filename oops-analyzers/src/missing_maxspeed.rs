//! Missing Maxspeed Analyzer
//!
//! ## Overview
//!
//! This analyzer identifies main roads that are missing the `maxspeed` tag in OpenStreetMap data.
//! Speed limit information is crucial for navigation systems, traffic simulation, and routing
//! applications. Main roads without explicit speed limits can cause navigation uncertainty
//! and inconsistent routing behavior.
//!
//! ## What it Analyzes
//!
//! The analyzer examines all road ways in the query area and identifies those that:
//! 1. Are considered "main roads" (excludes residential, unclassified, and service roads)
//! 2. Are missing the `maxspeed` tag
//! 3. Are not special road types (links, ramps) that may inherit speed limits
//!
//! ## Road Classification
//!
//! The analyzer considers these highway types as "main roads":
//! - `primary` - Major roads, often with national significance
//! - `secondary` - Important roads with regional significance  
//! - `tertiary` - Roads connecting smaller settlements or neighborhoods
//! - `trunk` - Important non-motorway roads
//! - `motorway` - Controlled-access highways
//! For a discussion of road types in OSM, please consult the OpenStreetMap Wiki.
//!
//! The following highway types are **excluded** as they typically don't require explicit maxspeed:
//! - `residential` - Local residential streets (often have implicit speed limits)
//! - `unclassified` - Minor public roads
//! - `service` - Access roads to buildings, parking lots, etc.
//! - `*_link` roads - Connecting ramps that may inherit speed from connected roads
//!
//! ## Example Usage
//!
//! ```rust
//! use oops_core::*;
//! use oops_analyzers::MissingMaxspeedAnalyzer;
//!
//! // Create the analyzer
//! let analyzer = MissingMaxspeedAnalyzer::new();
//!
//! // Set up analysis area (bounding box around downtown area)
//! let bbox = BoundingBox::new(-111.9, 40.7, -111.8, 40.8);
//! let query = AnalysisQuery::new(bbox);
//!
//! // Run analysis on OSM data
//! let results = analyzer.analyze(&query, &osm_data);
//!
//! // Process results
//! for result in results {
//!     println!("Way {} is missing maxspeed tag", result.element_id);
//!     for finding in result.findings {
//!         println!("  Issue: {}", finding.message);
//!         if let Some(rec) = finding.recommendation {
//!             println!(("  Recommendation: {:?}", rec);
//!         }
//!     }
//! }
//! ```
//!
//! ## Configuration
//!
//! The analyzer accepts these configuration parameters:
//! - `include_trunk_links` (default: false) - Whether to include trunk_link roads
//! - `include_primary_links` (default: false) - Whether to include primary_link roads
//!
//! ```rust
//! let mut config = AnalyzerConfig::new("missing_maxspeed");
//! config.set("include_trunk_links", "true");
//! let analyzer = MissingMaxspeedAnalyzer::with_config(config);
//! ```

use oops_core::*;
use std::collections::HashMap;

/// Analyzer that identifies main roads missing maxspeed tags
///
/// This is a simple, non-spatial analyzer that examines each road way individually
/// without considering spatial relationships or intersections. It serves as a good
/// example of a straightforward OSM data quality analyzer.
///
/// ## Design Principles
///
/// 1. **Simple Logic**: Each way is analyzed independently
/// 2. **Clear Categorization**: Explicit inclusion/exclusion rules for road types
/// 3. **Actionable Results**: Each finding includes specific recommendations
/// 4. **Configurable**: Allows customization of which road types to include
/// 5. **Well Documented**: Extensive comments to help developers understand the pattern
pub struct MissingMaxspeedAnalyzer {
    /// Configuration for the analyzer behavior
    config: AnalyzerConfig,
}

impl MissingMaxspeedAnalyzer {
    /// Create a new analyzer with default settings
    ///
    /// Default configuration:
    /// - Does not include link roads (ramps, connectors)
    /// - Focuses on main through-roads only
    ///
    /// # Returns
    /// New analyzer instance ready to use
    pub fn new() -> Self {
        Self {
            config: AnalyzerConfig::new("missing_maxspeed"),
        }
    }

    /// Create a new analyzer with custom configuration
    ///
    /// # Arguments
    /// * `config` - Configuration object with analyzer parameters
    ///
    /// # Returns
    /// New analyzer instance with specified configuration
    ///
    /// # Example
    /// ```rust
    /// let mut config = AnalyzerConfig::new("missing_maxspeed");
    /// config.set("include_trunk_links", "true");
    /// let analyzer = MissingMaxspeedAnalyzer::with_config(config);
    /// ```
    pub fn with_config(config: AnalyzerConfig) -> Self {
        Self { config }
    }

    /// Check if a road way should be analyzed for missing maxspeed
    ///
    /// This is the core logic that determines which roads are considered "main roads"
    /// that should have explicit speed limits.
    ///
    /// # Arguments
    /// * `way` - The OSM way to examine
    ///
    /// # Returns
    /// `true` if this way should be checked for maxspeed, `false` otherwise
    ///
    /// # Logic Details
    ///
    /// **Included road types:**
    /// - primary, secondary, tertiary (main through roads)
    /// - trunk, motorway (high-speed roads)
    /// - Optionally: link roads if configured
    ///
    /// **Excluded road types:**
    /// - residential (local streets with implicit speed limits)
    /// - unclassified (minor roads)
    /// - service (driveways, parking lots)
    /// - footway, cycleway (non-vehicle infrastructure)
    /// - construction, proposed (incomplete roads)
    fn should_analyze_way(&self, way: &OsmWay) -> bool {
        // Get the highway tag - if missing, this isn't a road
        let highway_type = match way.tags.get("highway") {
            Some(highway) => highway,
            None => return false, // Not a road way
        };

        // Define the main road types that should have explicit speed limits
        let main_road_types = [
            "primary",   // Major roads, often national significance
            "secondary", // Important regional roads
            "tertiary",  // Roads connecting smaller settlements
            "trunk",     // Important non-motorway roads
            "motorway",  // Controlled-access highways
        ];

        // Check if this is a main road type
        if main_road_types.contains(&highway_type.as_str()) {
            return true;
        }

        // Handle link roads based on configuration
        // Link roads are connectors/ramps that may inherit speed from main roads
        if highway_type.ends_with("_link") {
            // Check if we should include specific types of link roads
            let include_trunk_links = self
                .config
                .get("include_trunk_links")
                .map_or(false, |v| v == "true");

            let include_primary_links = self
                .config
                .get("include_primary_links")
                .map_or(false, |v| v == "true");

            return match highway_type.as_str() {
                "trunk_link" => include_trunk_links,
                "primary_link" => include_primary_links,
                "motorway_link" => true, // Always include motorway links
                _ => false,
            };
        }

        // Explicitly exclude these road types as they typically don't need maxspeed
        // or are not suitable for vehicle traffic
        false // If not in the included list above, exclude by default
    }

    /// Check if a way has an explicit maxspeed tag
    ///
    /// # Arguments
    /// * `way` - The OSM way to check
    ///
    /// # Returns
    /// `true` if the way has a maxspeed tag, `false` otherwise
    ///
    /// # Recognized Maxspeed Tags
    ///
    /// This function looks for the standard `maxspeed` tag. Future versions
    /// could be enhanced to recognize:
    /// - `maxspeed:forward` / `maxspeed:backward` (directional speeds)
    /// - `maxspeed:conditional` (conditional speed limits)
    /// - `source:maxspeed` (implicit speeds like "DE:urban")
    fn has_maxspeed_tag(&self, way: &OsmWay) -> bool {
        way.tags.contains_key("maxspeed")
    }

    /// Create detailed context information for a finding
    ///
    /// Context provides additional structured data about the issue that can be
    /// used by downstream tools or for detailed reporting.
    ///
    /// # Arguments
    /// * `way` - The way with the missing maxspeed tag
    ///
    /// # Returns
    /// HashMap with context key-value pairs
    ///
    /// # Context Fields
    ///
    /// - `way_id` - The OSM way ID as a string
    /// - `highway_type` - The highway classification
    /// - `way_name` - The road name (if available)
    /// - `way_ref` - The road reference number (if available)
    /// - `node_count` - Number of nodes in the way (indicates length/complexity)
    fn create_finding_context(&self, way: &OsmWay) -> HashMap<String, String> {
        let mut context = HashMap::new();

        // Always include the way ID for reference
        context.insert("way_id".into(), way.id.to_string());

        // Include the highway type for categorization
        if let Some(highway_type) = way.tags.get("highway") {
            context.insert("highway_type".into(), highway_type.clone());
        }

        // Include road name if available (helps with human-readable reporting)
        if let Some(name) = way.tags.get("name") {
            context.insert("way_name".into(), name.clone());
        }

        // Include road reference number if available (like "US-101", "A1")
        if let Some(road_ref) = way.tags.get("ref") {
            context.insert("way_ref".into(), road_ref.clone());
        }

        // Include node count as an indicator of way length/complexity
        context.insert("node_count".into(), way.nodes.len().to_string());

        context
    }

    /// Generate a human-readable message for a missing maxspeed finding
    ///
    /// # Arguments
    /// * `way` - The way missing the maxspeed tag
    ///
    /// # Returns
    /// A descriptive message string
    ///
    /// # Message Format
    ///
    /// The message includes:
    /// 1. The highway type for context
    /// 2. The way name or ID for identification
    /// 3. Brief explanation of why this matters
    fn create_finding_message(&self, way: &OsmWay) -> String {
        let default_highway = "road".into();
        let highway_type = way.tags.get("highway").unwrap_or(&default_highway);

        // Use road name if available, otherwise use way ID
        let road_identifier = way
            .tags
            .get("name")
            .map(|name| format!("\"{}\"", name))
            .unwrap_or_else(|| format!("way {}", way.id));

        format!(
            "{} {} is missing maxspeed tag. This can cause routing inconsistencies and navigation uncertainty.",
            highway_type.replace("_", " ").to_uppercase(),
            road_identifier
        )
    }

    /// Generate actionable recommendation for fixing the issue
    ///
    /// # Arguments
    /// * `way` - The way missing the maxspeed tag
    ///
    /// # Returns
    /// A recommendation string with specific guidance
    ///
    /// # Recommendation Content
    ///
    /// Provides specific guidance based on road type and available information.
    /// Could be enhanced to include region-specific advice or typical speed
    /// ranges for different road types.
    fn create_recommendation(&self, way: &OsmWay) -> String {
        let default_highway = "road".into();
        let highway_type = way.tags.get("highway").unwrap_or(&default_highway);

        match highway_type.as_str() {
            "motorway" => {
                "Add maxspeed tag with the posted speed limit (typically 55-80+ mph/90-130+ km/h depending on country). Check roadside signs or official speed limit maps.".into()
            }
            "trunk" => {
                "Add maxspeed tag with the posted speed limit (typically 45-70 mph/70-110 km/h). Verify with local signage or transportation authority data.".into()
            }
            "primary" | "secondary" => {
                "Add maxspeed tag with the posted speed limit (typically 25-55 mph/40-90 km/h). Check posted signs along the road.".into()
            }
            "tertiary" => {
                "Add maxspeed tag with the posted speed limit (typically 25-45 mph/40-70 km/h). Look for speed limit signs at road entry points.".into()
            }
            _ => {
                "Add maxspeed tag with the posted speed limit. Check roadside signage or local traffic regulations for the correct value.".into()
            }
        }
    }

    /// Determine the severity level for a missing maxspeed finding
    ///
    /// # Arguments
    /// * `way` - The way missing the maxspeed tag
    ///
    /// # Returns
    /// Severity level based on road importance
    ///
    /// # Severity Logic
    ///
    /// - **High**: Motorways and trunks (high-speed, high-traffic roads)
    /// - **Medium**: Primary and secondary roads (main arterials)
    /// - **Low**: Tertiary and other roads (lower traffic)
    ///
    /// Higher severity indicates more impact on navigation and routing systems.
    fn determine_severity(&self, way: &OsmWay) -> Severity {
        let default_highway = "unknown".into();
        let highway_type = way.tags.get("highway").unwrap_or(&default_highway);

        match highway_type.as_str() {
            "motorway" | "trunk" => Severity::High,
            "primary" | "secondary" => Severity::Medium,
            _ => Severity::Low,
        }
    }
}

// Implement the core Analyzer trait
// This trait defines the interface that all analyzers must implement
impl Analyzer for MissingMaxspeedAnalyzer {
    /// Return the unique identifier for this analyzer
    ///
    /// This name is used for:
    /// - Registering the analyzer in the system
    /// - Configuration file references
    /// - Command-line tool selection
    /// - API endpoint routing
    fn name(&self) -> &str {
        "missing_maxspeed"
    }

    /// Return the version of this analyzer
    ///
    /// Used for:
    /// - Compatibility checking
    /// - Result provenance tracking
    /// - API versioning
    fn version(&self) -> &str {
        "1.0.0"
    }

    /// Return a human-readable description of what this analyzer does
    ///
    /// This description appears in:
    /// - Help documentation
    /// - API responses
    /// - User interfaces
    /// - Configuration tools
    fn description(&self) -> &str {
        "Identifies main roads (primary, secondary, tertiary, trunk, motorway) that are missing maxspeed tags, which can cause navigation and routing inconsistencies"
    }

    /// Perform the actual analysis on OSM data
    ///
    /// This is the core method that implements the analyzer's logic.
    ///
    /// # Arguments
    /// * `query` - Defines the area and parameters for analysis
    /// * `data` - OSM data to analyze (nodes, ways, relations)
    ///
    /// # Returns
    /// Vector of analysis results, one per problematic way found
    ///
    /// # Analysis Process
    ///
    /// 1. **Filter ways**: Only examine ways within the query bounding box
    /// 2. **Check relevance**: Apply `should_analyze_way()` to filter road types
    /// 3. **Check maxspeed**: Look for missing maxspeed tags
    /// 4. **Create findings**: Generate detailed results with context and recommendations
    /// 5. **Return results**: Package everything for the caller
    ///
    /// # Performance Considerations
    ///
    /// - This analyzer is O(n) where n is the number of ways
    /// - No spatial indexing needed (simple tag checking)
    /// - Memory usage is proportional to number of findings
    /// - Can process large datasets efficiently
    fn analyze(&self, query: &AnalysisQuery, data: &OsmData) -> Vec<AnalysisResult> {
        let mut results = Vec::new();

        // Iterate through all ways in the dataset
        // Note: We could optimize this by using spatial indexing to only examine
        // ways that intersect the query bounding box, but for simplicity we'll
        // check all ways and filter by bbox later
        for way in data.ways.values() {
            // Step 1: Check if this way is a type we should analyze
            if !self.should_analyze_way(way) {
                continue; // Skip ways that aren't main roads
            }

            // Step 2: Check if this way already has a maxspeed tag
            if self.has_maxspeed_tag(way) {
                continue; // Skip ways that already have speed limits
            }

            // Step 3: Check if any part of this way is within our query area
            // For this simple analyzer, we'll check if any node of the way
            // is within the bounding box. A more sophisticated approach would
            // check if the way geometry intersects the bbox.
            let way_in_query_area = way.nodes.iter().any(|&node_id| {
                if let Some(node) = data.get_node(node_id) {
                    query.area.contains_point(node.lon, node.lat)
                } else {
                    false // Node not found in dataset
                }
            });

            if !way_in_query_area {
                continue; // Skip ways outside our analysis area
            }

            // Step 4: Create an analysis result for this problematic way
            let mut result = AnalysisResult::new(way.id, ElementType::Way);

            // Step 5: Create a detailed finding with context and recommendations
            let finding = Finding {
                category: ProblemCategory::Missing, // This is a missing data issue
                severity: self.determine_severity(way), // Severity based on road type
                message: Some(self.create_finding_message(way)), // Human-readable description
                recommendation: Some(self.create_recommendation(way)), // Actionable advice
                context: self.create_finding_context(way), // Structured metadata
            };

            // Add the finding to our result
            result.add_finding(finding);

            // Add this result to our collection
            results.push(result);
        }

        // Return all findings
        // Results are sorted by way ID for consistent ordering
        results.sort_by_key(|r| r.element_id);
        results
    }
}

// Unit tests demonstrate how the analyzer works and serve as examples
#[cfg(test)]
mod tests {
    use super::*;

    /// Create test data with various road types for testing
    ///
    /// This helper function creates a realistic dataset with:
    /// - Roads that should be analyzed (primary, secondary, etc.)
    /// - Roads that should be ignored (residential, service, etc.)
    /// - Some roads with maxspeed tags, some without
    fn create_test_data() -> OsmData {
        let mut data = OsmData::new();

        // Add nodes for our test roads
        data.add_node(OsmNode::new(1, -111.9, 40.75)); // Primary road start
        data.add_node(OsmNode::new(2, -111.85, 40.75)); // Primary road end
        data.add_node(OsmNode::new(3, -111.9, 40.76)); // Secondary road start
        data.add_node(OsmNode::new(4, -111.85, 40.76)); // Secondary road end
        data.add_node(OsmNode::new(5, -111.9, 40.77)); // Residential road start
        data.add_node(OsmNode::new(6, -111.85, 40.77)); // Residential road end

        // Way 1: Primary road WITHOUT maxspeed (should be flagged)
        let mut primary_way = OsmWay::new(1, vec![1, 2]);
        primary_way.tags.insert("highway".into(), "primary".into());
        primary_way.tags.insert("name".into(), "Main Street".into());
        primary_way.tags.insert("ref".into(), "US-89".into());
        data.ways.insert(1, primary_way);

        // Way 2: Secondary road WITH maxspeed (should NOT be flagged)
        let mut secondary_way = OsmWay::new(2, vec![3, 4]);
        secondary_way
            .tags
            .insert("highway".into(), "secondary".into());
        secondary_way
            .tags
            .insert("name".into(), "Oak Avenue".into());
        secondary_way
            .tags
            .insert("maxspeed".into(), "45 mph".into());
        data.ways.insert(2, secondary_way);

        // Way 3: Residential road WITHOUT maxspeed (should NOT be flagged)
        let mut residential_way = OsmWay::new(3, vec![5, 6]);
        residential_way
            .tags
            .insert("highway".into(), "residential".into());
        residential_way
            .tags
            .insert("name".into(), "Elm Street".into());
        data.ways.insert(3, residential_way);

        data
    }

    /// Test basic analyzer creation and metadata
    #[test]
    fn test_analyzer_creation() {
        let analyzer = MissingMaxspeedAnalyzer::new();

        // Verify metadata
        assert_eq!(analyzer.name(), "missing_maxspeed");
        assert_eq!(analyzer.version(), "1.0.0");
        assert!(analyzer.description().contains("maxspeed"));
        assert!(analyzer.description().contains("main roads"));
    }

    /// Test which road types should be analyzed
    #[test]
    fn test_should_analyze_way() {
        let analyzer = MissingMaxspeedAnalyzer::new();

        // Test main road types (should be analyzed)
        let main_road_types = ["primary", "secondary", "tertiary", "trunk", "motorway"];
        for highway_type in &main_road_types {
            let mut way = OsmWay::new(1, vec![1, 2]);
            way.tags.insert("highway".into(), highway_type.into());

            assert!(
                analyzer.should_analyze_way(&way),
                "{} roads should be analyzed",
                highway_type
            );
        }

        // Test excluded road types (should NOT be analyzed)
        let excluded_types = ["residential", "unclassified", "service", "footway"];
        for highway_type in &excluded_types {
            let mut way = OsmWay::new(1, vec![1, 2]);
            way.tags.insert("highway".into(), highway_type.into());

            assert!(
                !analyzer.should_analyze_way(&way),
                "{} roads should not be analyzed",
                highway_type
            );
        }

        // Test non-road ways (no highway tag)
        let mut non_road = OsmWay::new(1, vec![1, 2]);
        non_road.tags.insert("natural".into(), "water".into());
        assert!(!analyzer.should_analyze_way(&non_road));
    }

    /// Test link road configuration
    #[test]
    fn test_link_road_configuration() {
        // Default configuration should exclude most link roads
        let default_analyzer = MissingMaxspeedAnalyzer::new();

        let mut trunk_link = OsmWay::new(1, vec![1, 2]);
        trunk_link
            .tags
            .insert("highway".into(), "trunk_link".into());
        assert!(!default_analyzer.should_analyze_way(&trunk_link));

        // Configure to include trunk links
        let mut config = AnalyzerConfig::new("missing_maxspeed");
        config.set("include_trunk_links", "true");
        let configured_analyzer = MissingMaxspeedAnalyzer::with_config(config);

        assert!(configured_analyzer.should_analyze_way(&trunk_link));

        // Motorway links should always be included
        let mut motorway_link = OsmWay::new(2, vec![1, 2]);
        motorway_link
            .tags
            .insert("highway".into(), "motorway_link".into());
        assert!(default_analyzer.should_analyze_way(&motorway_link));
    }

    /// Test maxspeed tag detection
    #[test]
    fn test_has_maxspeed_tag() {
        let analyzer = MissingMaxspeedAnalyzer::new();

        // Way with maxspeed tag
        let mut way_with_speed = OsmWay::new(1, vec![1, 2]);
        way_with_speed
            .tags
            .insert("maxspeed".into(), "50 mph".into());
        assert!(analyzer.has_maxspeed_tag(&way_with_speed));

        // Way without maxspeed tag
        let way_without_speed = OsmWay::new(2, vec![1, 2]);
        assert!(!analyzer.has_maxspeed_tag(&way_without_speed));
    }

    /// Test severity determination based on road type
    #[test]
    fn test_severity_determination() {
        let analyzer = MissingMaxspeedAnalyzer::new();

        // High severity roads
        let mut motorway = OsmWay::new(1, vec![1, 2]);
        motorway.tags.insert("highway".into(), "motorway".into());
        assert_eq!(analyzer.determine_severity(&motorway), Severity::High);

        let mut trunk = OsmWay::new(2, vec![1, 2]);
        trunk.tags.insert("highway".into(), "trunk".into());
        assert_eq!(analyzer.determine_severity(&trunk), Severity::High);

        // Medium severity roads
        let mut primary = OsmWay::new(3, vec![1, 2]);
        primary.tags.insert("highway".into(), "primary".into());
        assert_eq!(analyzer.determine_severity(&primary), Severity::Medium);

        // Low severity roads
        let mut tertiary = OsmWay::new(4, vec![1, 2]);
        tertiary.tags.insert("highway".into(), "tertiary".into());
        assert_eq!(analyzer.determine_severity(&tertiary), Severity::Low);
    }

    /// Test the complete analysis workflow
    #[test]
    fn test_analysis_workflow() {
        let data = create_test_data();
        let analyzer = MissingMaxspeedAnalyzer::new();

        // Create a query covering our test area
        let bbox = BoundingBox::new(-111.95, 40.74, -111.8, 40.78);
        let query = AnalysisQuery::new(bbox);

        // Run the analysis
        let results = analyzer.analyze(&query, &data);

        // Should find exactly one problem: the primary road without maxspeed
        assert_eq!(results.len(), 1, "Should find one road missing maxspeed");

        let result = &results[0];
        assert_eq!(result.element_id, 1); // Primary road way ID
        assert_eq!(result.element_type, ElementType::Way);
        assert_eq!(result.findings.len(), 1);

        let finding = &result.findings[0];
        assert_eq!(finding.category, ProblemCategory::Missing);
        assert_eq!(finding.severity, Severity::Medium); // Primary road = medium severity
        assert!(finding.message.as_ref().unwrap().contains("PRIMARY"));
        assert!(finding.message.as_ref().unwrap().contains("Main Street"));

        // Check recommendation is present and helpful
        assert!(finding.recommendation.is_some());
        let recommendation = finding.recommendation.as_ref().unwrap();
        assert!(recommendation.contains("maxspeed"));
        assert!(recommendation.contains("posted"));

        // Check context includes useful metadata
        assert_eq!(finding.context.get("way_id"), Some(&"1".into()));
        assert_eq!(finding.context.get("highway_type"), Some(&"primary".into()));
        assert_eq!(finding.context.get("way_name"), Some(&"Main Street".into()));
        assert_eq!(finding.context.get("way_ref"), Some(&"US-89".into()));
    }

    /// Test that roads outside the query area are ignored
    #[test]
    fn test_spatial_filtering() {
        let data = create_test_data();
        let analyzer = MissingMaxspeedAnalyzer::new();

        // Create a query that doesn't cover our test roads
        let bbox = BoundingBox::new(-112.0, 40.0, -111.99, 40.1);
        let query = AnalysisQuery::new(bbox);

        let results = analyzer.analyze(&query, &data);

        // Should find no results since roads are outside the area
        assert_eq!(results.len(), 0, "Should find no roads outside query area");
    }

    /// Test finding context creation
    #[test]
    fn test_finding_context() {
        let analyzer = MissingMaxspeedAnalyzer::new();

        let mut way = OsmWay::new(123, vec![1, 2, 3, 4]);
        way.tags.insert("highway".into(), "primary".into());
        way.tags.insert("name".into(), "Test Road".into());
        way.tags.insert("ref".into(), "SR-456".into());

        let context = analyzer.create_finding_context(&way);

        assert_eq!(context.get("way_id"), Some(&"123".into()));
        assert_eq!(context.get("highway_type"), Some(&"primary".into()));
        assert_eq!(context.get("way_name"), Some(&"Test Road".into()));
        assert_eq!(context.get("way_ref"), Some(&"SR-456".into()));
        assert_eq!(context.get("node_count"), Some(&"4".into()));
    }

    /// Test message generation for different road types
    #[test]
    fn test_message_generation() {
        let analyzer = MissingMaxspeedAnalyzer::new();

        // Test named road
        let mut named_way = OsmWay::new(1, vec![1, 2]);
        named_way.tags.insert("highway".into(), "primary".into());
        named_way.tags.insert("name".into(), "Main Street".into());

        let message = analyzer.create_finding_message(&named_way);
        assert!(message.contains("PRIMARY"));
        assert!(message.contains("\"Main Street\""));
        assert!(message.contains("maxspeed"));

        // Test unnamed road (should use way ID)
        let mut unnamed_way = OsmWay::new(2, vec![1, 2]);
        unnamed_way
            .tags
            .insert("highway".into(), "secondary".into());

        let message = analyzer.create_finding_message(&unnamed_way);
        assert!(message.contains("SECONDARY"));
        assert!(message.contains("way 2"));
    }
}
