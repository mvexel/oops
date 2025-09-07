//! Spatial analysis utilities for OpenStreetMap data.
//!
//! This module contains functions for geometric calculations, proximity analysis,
//! and spatial relationship checks (e.g., parallel ways, sidewalk coverage).

use crate::data::{OsmData, OsmWay};
use geo::{
    Coord, GeodesicLength, HaversineBearing, HaversineDistance, LineInterpolatePoint, LineString,
    Point,
};
use rstar::{RTree, RTreeObject, AABB};

#[derive(Debug, Clone)]
pub struct WayWithLineString {
    pub way: OsmWay,
    pub linestring: LineString,
}

impl RTreeObject for WayWithLineString {
    type Envelope = AABB<Point>;

    fn envelope(&self) -> Self::Envelope {
        self.linestring.envelope()
    }
}

/// Finds ways from a given collection of candidates that are spatially close to a target way.
pub fn find_adjacent_way_candidates<'a>(
    target_way: &OsmWay,
    candidate_rtree: &'a RTree<WayWithLineString>,
    max_distance: f64,
    data: &OsmData,
) -> Vec<&'a WayWithLineString> {
    if max_distance <= 0.0 || max_distance > 1000.0 {
        return Vec::new();
    }

    if let Some(target_linestring) = way_to_linestring(target_way, data) {
        let target_envelope = target_linestring.envelope();
        let buffer_distance = max_distance / 111_320.0; // Rough conversion from meters to degrees
        let bigger_envelope: AABB<Point> = AABB::from_corners(
            (
                target_envelope.lower().x() - buffer_distance,
                target_envelope.lower().y() - buffer_distance,
            )
                .into(),
            (
                target_envelope.upper().x() + buffer_distance,
                target_envelope.upper().y() + buffer_distance,
            )
                .into(),
        );
        candidate_rtree
            .locate_in_envelope_intersecting(&bigger_envelope)
            .filter(|candidate| {
                let min_dist =
                    calculate_linestring_to_linestring_distance(&candidate.linestring, &target_linestring);
                min_dist.is_finite() && min_dist <= max_distance
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Checks if two ways have parallel segments by comparing bearings of individual segments.
pub fn are_ways_parallel(way1: &LineString, way2: &LineString, max_angle_diff: f64) -> bool {
    let way1_points: Vec<_> = way1.points().collect();
    let way2_points: Vec<_> = way2.points().collect();

    if way1_points.len() < 2 || way2_points.len() < 2 {
        return false;
    }

    if !(0.0..=180.0).contains(&max_angle_diff) {
        return false;
    }

    if way1_points.len() <= 3 && way2_points.len() <= 3 {
        return are_ways_parallel_simple(way1, way2, max_angle_diff);
    }

    let way1_segments = calculate_segment_bearings(&way1_points);
    let way2_segments = calculate_segment_bearings(&way2_points);

    if way1_segments.is_empty() || way2_segments.is_empty() {
        return false;
    }

    let mut parallel_score = 0.0;
    for (way1_bearing, way1_length) in &way1_segments {
        for (way2_bearing, way2_length) in &way2_segments {
            let angle_diff = (way1_bearing - way2_bearing).abs();
            let is_parallel =
                angle_diff <= max_angle_diff || (angle_diff - 180.0).abs() <= max_angle_diff;

            if is_parallel {
                let weight = (way1_length + way2_length) / 2.0;
                parallel_score += weight;
            }
        }
    }

    let total_way1_length: f64 = way1_segments.iter().map(|(_, length)| length).sum();
    if total_way1_length == 0.0 { return false; }
    let parallel_ratio = parallel_score / total_way1_length;
    parallel_ratio > 0.3
}

/// Calculate bearing and length for each segment of a way.
fn calculate_segment_bearings(points: &[geo::Point]) -> Vec<(f64, f64)> {
    points.windows(2).filter_map(|p| {
        if p[0] == p[1] {
            None
        } else {
            let bearing = p[0].haversine_bearing(p[1]);
            let length = p[0].haversine_distance(&p[1]);
            Some((bearing, length))
        }
    }).collect()
}

/// Simple bearing comparison for short ways (fallback method).
fn are_ways_parallel_simple(way1: &LineString, way2: &LineString, max_angle_diff: f64) -> bool {
    let (Some(way1_start), Some(way1_end)) = (way1.points().next(), way1.points().last()) else { return false; };
    let (Some(way2_start), Some(way2_end)) = (way2.points().next(), way2.points().last()) else { return false; };

    if way1_start == way1_end || way2_start == way2_end {
        return false;
    }

    let way1_bearing = way1_start.haversine_bearing(way1_end);
    let way2_bearing = way2_start.haversine_bearing(way2_end);
    let angle_diff = (way1_bearing - way2_bearing).abs();

    angle_diff <= max_angle_diff || (angle_diff - 180.0).abs() <= max_angle_diff
}

/// Finds ways that are both spatially close and roughly parallel to a target way.
pub fn find_parallel_adjacent_ways<'a>(
    target_way: &OsmWay,
    candidate_rtree: &'a RTree<WayWithLineString>,
    max_distance: f64,
    max_angle_diff: f64,
    data: &OsmData,
) -> Vec<&'a WayWithLineString> {
    if let Some(target_linestring) = way_to_linestring(target_way, data) {
        find_adjacent_way_candidates(target_way, candidate_rtree, max_distance, data)
            .into_iter()
            .filter(|candidate| {
                are_ways_parallel(&target_linestring, &candidate.linestring, max_angle_diff)
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Calculates what portion of a target way is covered by nearby parallel sidewalks.
pub fn calculate_sidewalk_coverage(
    target_way: &OsmWay,
    parallel_sidewalks: &[&WayWithLineString],
    max_distance: f64,
    data: &OsmData,
) -> f64 {
    if let Some(target_linestring) = way_to_linestring(target_way, data) {
        let target_points: Vec<_> = target_linestring.points().collect();
        if target_points.len() < 2 {
            return 0.0;
        }

        let sample_count = (target_linestring.geodesic_length() / 10.0).ceil().max(10.0) as usize;
        let mut covered_samples = 0;

        for i in 0..sample_count {
            let t = i as f64 / (sample_count - 1) as f64;
            let sample_point = target_linestring.line_interpolate_point(t).unwrap();

            let has_coverage = parallel_sidewalks.iter().any(|sidewalk| {
                let min_dist =
                    calculate_point_to_linestring_distance(&sample_point, &sidewalk.linestring);
                min_dist <= max_distance
            });

            if has_coverage {
                covered_samples += 1;
            }
        }

        covered_samples as f64 / sample_count as f64
    } else {
        0.0
    }
}

/// Calculate the minimum distance from a point to a LineString using point-to-segment distance.
pub fn calculate_point_to_linestring_distance(
    point: &geo::Point,
    linestring: &geo::LineString,
) -> f64 {
    use geo::HaversineDistance;
    linestring
        .lines()
        .map(|line| {
            // This is a simplified distance check. For higher accuracy, especially on long segments,
            // a proper point-to-great-circle-segment distance would be needed.
            // However, for typical OSM data, this is a reasonable approximation.
            let d1 = point.haversine_distance(&line.start_point());
            let d2 = point.haversine_distance(&line.end_point());
            d1.min(d2)
        })
        .fold(f64::MAX, f64::min)
}

/// Calculate the minimum distance between two LineStrings.
pub fn calculate_linestring_to_linestring_distance(
    linestring1: &geo::LineString,
    linestring2: &geo::LineString,
) -> f64 {
    linestring1.points().map(|p1| {
        calculate_point_to_linestring_distance(&p1, linestring2)
    }).fold(f64::MAX, f64::min)
}

/// Convert an OSM way to a LineString geometry.
pub fn way_to_linestring(way: &OsmWay, data: &OsmData) -> Option<LineString> {
    let coords: Result<Vec<Coord>, _> = way
        .nodes
        .iter()
        .map(|&node_id| {
            data.nodes
                .get(&node_id)
                .map(|node| Coord {
                    x: node.lon,
                    y: node.lat,
                })
                .ok_or(())
        })
        .collect();

    coords.ok().map(LineString::from)
}

