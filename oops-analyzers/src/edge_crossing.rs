//! Detects road ways that cross geometrically without a shared node or layer separation.

use geo::line_intersection::{line_intersection, LineIntersection};
use geo::Point;
use oops_core::analysis::*;
use oops_core::data::{OsmData, OsmWay};
use oops_core::spatial::way_to_linestring;
use std::collections::{HashMap, HashSet};

const ROAD_TYPES: &[&str] = &[
    "motorway",
    "trunk",
    "primary",
    "secondary",
    "tertiary",
    "unclassified",
    "residential",
    "motorway_link",
    "trunk_link",
    "primary_link",
    "secondary_link",
    "tertiary_link",
];

pub fn road_types() -> Vec<&'static str> {
    ROAD_TYPES.to_vec()
}

/// Finds road candidates whose geometries intersect the selected road way.
pub struct EdgeCrossingContextProvider {
    max_distance: f64,
}

impl EdgeCrossingContextProvider {
    pub fn new() -> Self {
        Self { max_distance: 0.1 }
    }
}

impl Default for EdgeCrossingContextProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextProvider for EdgeCrossingContextProvider {
    fn enrich_context(
        &self,
        element: &SelectedElement,
        spatial_index: &AnalysisSpatialIndex,
    ) -> EnrichedContext {
        let mut metadata = HashMap::new();

        if let Some(way) = spatial_index.osm_data().ways.get(&element.element_id) {
            let candidate_ids =
                spatial_index.find_intersecting_way_candidates(way, self.max_distance);
            metadata.insert(
                "candidate_way_ids".to_string(),
                candidate_ids
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }

        EnrichedContext {
            element: element.clone(),
            spatial_context: SpatialContextData::default(),
            metadata,
        }
    }
}

/// Reports unsplit crossing roads that are not explicitly separated by layer tags.
pub struct EdgeCrossingDetector;

impl EdgeCrossingDetector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EdgeCrossingDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProblemDetector for EdgeCrossingDetector {
    fn detect_problems(&self, context: &EnrichedContext, data: &OsmData) -> Vec<Finding> {
        let Some(target_way) = data.ways.get(&context.element.element_id) else {
            return Vec::new();
        };

        context
            .metadata
            .get("candidate_way_ids")
            .map(|ids| parse_candidate_ids(ids))
            .unwrap_or_default()
            .into_iter()
            .filter(|candidate_id| context.element.element_id < *candidate_id)
            .filter_map(|candidate_id| {
                let candidate_way = data.ways.get(&candidate_id)?;

                if ways_share_node(target_way, candidate_way)
                    || has_explicit_layer_separation(target_way, candidate_way)
                {
                    return None;
                }

                let crossing_points =
                    crossing_points_without_overlap(target_way, candidate_way, data);
                if crossing_points.is_empty() {
                    return None;
                }

                Some(build_finding(target_way, candidate_way, &crossing_points))
            })
            .collect()
    }
}

fn parse_candidate_ids(ids: &str) -> Vec<i64> {
    ids.split(',').filter_map(|id| id.parse().ok()).collect()
}

fn ways_share_node(way1: &OsmWay, way2: &OsmWay) -> bool {
    let (smaller, larger) = if way1.nodes.len() <= way2.nodes.len() {
        (&way1.nodes, &way2.nodes)
    } else {
        (&way2.nodes, &way1.nodes)
    };
    let node_ids: HashSet<_> = smaller.iter().collect();
    larger.iter().any(|node| node_ids.contains(node))
}

fn has_explicit_layer_separation(way1: &OsmWay, way2: &OsmWay) -> bool {
    let layer1 = way1
        .tags
        .get("layer")
        .and_then(|value| value.parse::<i32>().ok());
    let layer2 = way2
        .tags
        .get("layer")
        .and_then(|value| value.parse::<i32>().ok());

    matches!((layer1, layer2), (Some(a), Some(b)) if a != b)
        || matches!((layer1, layer2), (Some(a), None) if a != 0)
        || matches!((layer1, layer2), (None, Some(b)) if b != 0)
}

fn crossing_points_without_overlap(way1: &OsmWay, way2: &OsmWay, data: &OsmData) -> Vec<Point> {
    let Some(linestring1) = way_to_linestring(way1, data) else {
        return Vec::new();
    };
    let Some(linestring2) = way_to_linestring(way2, data) else {
        return Vec::new();
    };

    let mut crossings = Vec::new();
    for line1 in linestring1.lines() {
        for line2 in linestring2.lines() {
            if let Some(LineIntersection::SinglePoint { intersection, .. }) =
                line_intersection(line1, line2)
            {
                crossings.push(Point::from(intersection));
            }
        }
    }

    crossings
}

fn build_finding(target_way: &OsmWay, crossing_way: &OsmWay, crossing_points: &[Point]) -> Finding {
    let first_point = crossing_points[0];
    let mut context = HashMap::new();
    context.insert("crossing_way_id".to_string(), crossing_way.id.to_string());
    context.insert(
        "crossing_count".to_string(),
        crossing_points.len().to_string(),
    );
    context.insert(
        "crossing_lon".to_string(),
        format!("{:.7}", first_point.x()),
    );
    context.insert(
        "crossing_lat".to_string(),
        format!("{:.7}", first_point.y()),
    );

    if let Some(highway) = target_way.tags.get("highway") {
        context.insert("target_highway".to_string(), highway.clone());
    }
    if let Some(highway) = crossing_way.tags.get("highway") {
        context.insert("crossing_highway".to_string(), highway.clone());
    }
    if let Some(layer) = target_way.tags.get("layer") {
        context.insert("target_layer".to_string(), layer.clone());
    }
    if let Some(layer) = crossing_way.tags.get("layer") {
        context.insert("crossing_layer".to_string(), layer.clone());
    }

    Finding {
        category: ProblemCategory::Invalid,
        severity: Severity::High,
        message: Some(format!(
            "Road crosses way {} without a shared node or explicit layer separation",
            crossing_way.id
        )),
        context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oops_core::{OsmData, OsmNode, OsmWay};

    fn test_data() -> OsmData {
        let mut data = OsmData::new();
        data.add_node(OsmNode::new(1, 0.0, 0.0));
        data.add_node(OsmNode::new(2, 1.0, 1.0));
        data.add_node(OsmNode::new(3, 0.0, 1.0));
        data.add_node(OsmNode::new(4, 1.0, 0.0));

        let mut way1 = OsmWay::new(10, vec![1, 2]);
        way1.tags
            .insert("highway".to_string(), "primary".to_string());
        let mut way2 = OsmWay::new(20, vec![3, 4]);
        way2.tags
            .insert("highway".to_string(), "secondary".to_string());

        data.ways.insert(way1.id, way1);
        data.ways.insert(way2.id, way2);
        data
    }

    #[test]
    fn finds_crossing_without_shared_node() {
        let data = test_data();
        let way1 = data.ways.get(&10).unwrap();
        let way2 = data.ways.get(&20).unwrap();

        let crossings = crossing_points_without_overlap(way1, way2, &data);

        assert_eq!(crossings.len(), 1);
        assert!((crossings[0].x() - 0.5).abs() < 0.000001);
        assert!((crossings[0].y() - 0.5).abs() < 0.000001);
    }

    #[test]
    fn explicit_different_layers_are_not_reported() {
        let mut data = test_data();
        data.ways
            .get_mut(&10)
            .unwrap()
            .tags
            .insert("layer".to_string(), "1".to_string());

        let way1 = data.ways.get(&10).unwrap();
        let way2 = data.ways.get(&20).unwrap();

        assert!(has_explicit_layer_separation(way1, way2));
    }

    #[test]
    fn detector_reports_candidate_crossing() {
        let data = test_data();
        let detector = EdgeCrossingDetector::new();
        let context = EnrichedContext {
            element: SelectedElement {
                element_id: 10,
                element_type: ElementType::Way,
                tags: data.ways.get(&10).unwrap().tags.clone(),
                geometry: ElementGeometry::LineString {
                    nodes: vec![(0.0, 0.0), (1.0, 1.0)],
                },
            },
            spatial_context: SpatialContextData::default(),
            metadata: HashMap::from([("candidate_way_ids".to_string(), "20".to_string())]),
        };

        let findings = detector.detect_problems(&context, &data);

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, ProblemCategory::Invalid);
        assert_eq!(
            findings[0].context.get("crossing_way_id"),
            Some(&"20".to_string())
        );
    }
}
