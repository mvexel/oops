//! Spatial analysis utilities for OpenStreetMap data.
//!
//! This module contains functions for geometric calculations, proximity analysis,
//! and spatial relationship checks (e.g., parallel ways, sidewalk coverage).

use crate::data::{OsmData, OsmWay};
use geo::algorithm::relate::Relate;
use geo::algorithm::validation::Validation;
use geo::line_intersection::{line_intersection, LineIntersection};
use geo::{
    algorithm::closest_point::ClosestPoint, line_measures::FrechetDistance, Bearing, Closest,
    Coord, CoordsIter, Distance, Euclidean, Geodesic, Haversine, InterpolatableLine, Length, Line,
    LineString, Point,
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
                let min_dist = calculate_linestring_to_linestring_distance(
                    &candidate.linestring,
                    &target_linestring,
                );
                min_dist.is_finite() && min_dist <= max_distance
            })
            .collect()
    } else {
        Vec::new()
    }
}

/// Decide if coordinates look like lon/lat (geographic).
fn is_geographic(ls: &LineString) -> bool {
    ls.0.iter().all(|c| c.x.abs() <= 180.0 && c.y.abs() <= 90.0)
}

/// Compute direction (bearing or vector) depending on CRS.
fn direction(line: &Line, geographic: bool) -> Option<f64> {
    if line.start == line.end {
        return None; // degenerate
    }
    if geographic {
        Some(Haversine.bearing(Point::from(line.start), Point::from(line.end)))
    } else {
        let dx = line.end.x - line.start.x;
        let dy = line.end.y - line.start.y;
        Some(dy.atan2(dx).to_degrees()) // angle in degrees
    }
}

/// Project one linestring onto another and compare bearings.
fn project_and_compare(shorter: &LineString, longer: &LineString) -> Option<f64> {
    let geographic = is_geographic(shorter) && is_geographic(longer);

    // Shorter segment (start to end)
    let short_seg = Line::new(shorter[0], shorter[shorter.0.len() - 1]);

    // Project shorter's endpoints onto longer
    let proj_start = match longer.closest_point(&Point::from(short_seg.start)) {
        Closest::SinglePoint(p) | Closest::Intersection(p) => p,
        _ => return None,
    };
    let proj_end = match longer.closest_point(&Point::from(short_seg.end)) {
        Closest::SinglePoint(p) | Closest::Intersection(p) => p,
        _ => return None,
    };
    let long_seg = Line::new(Coord::from(proj_start), Coord::from(proj_end));

    // Check for meaningful overlap
    let projected_length = if geographic {
        Haversine.distance(Point::from(proj_start), Point::from(proj_end))
    } else {
        Euclidean.length(&long_seg)
    };

    let original_length = if geographic {
        Haversine.distance(Point::from(short_seg.start), Point::from(short_seg.end))
    } else {
        Euclidean.length(&short_seg)
    };

    // Require meaningful overlap (at least 30% of original length)
    if original_length == 0.0 || projected_length / original_length < 0.3 {
        return None;
    }

    // If projections collapse, fallback to full longer line
    let effective_long_seg = if long_seg.start == long_seg.end {
        Line::new(longer[0], longer[longer.0.len() - 1])
    } else {
        long_seg
    };

    // Compute bearings / directions
    let dir_short = direction(&short_seg, geographic)?;
    let dir_long = direction(&effective_long_seg, geographic)?;

    // Angular difference, normalized to [0, 90]
    let mut diff = (dir_short - dir_long).abs();
    while diff > 180.0 {
        diff -= 360.0;
    }
    Some(diff.abs().min(180.0 - diff.abs()))
}

/// Checks if two ways have parallel segments using projection-based analysis.
/// This approach correctly rejects perpendicular crossings by projecting the shorter
/// linestring onto the longer one and comparing bearings of the original vs projected segments.
pub fn are_ways_geometrically_parallel(
    way1: &LineString,
    way2: &LineString,
    max_angle_diff: f64,
) -> bool {
    if way1.0.is_empty() || way2.0.is_empty() {
        return false;
    }

    if !(0.0..=180.0).contains(&max_angle_diff) {
        return false;
    }

    // First check if the lines cross - if they do, they cannot be parallel
    let intersection_matrix = way1.relate(way2);
    if intersection_matrix.is_crosses() {
        return false;
    }

    // Determine which is shorter vs longer
    let (shorter, longer) = if Euclidean.length(way1) <= Euclidean.length(way2) {
        (way1, way2)
    } else {
        (way2, way1)
    };

    // Try projecting shorter onto longer
    if let Some(angle_diff) = project_and_compare(shorter, longer) {
        // Debug output for the problematic case
        if shorter.0.first()
            == Some(&geo::Coord {
                x: -111.887135,
                y: 40.764907,
            })
            || longer.0.first()
                == Some(&geo::Coord {
                    x: -111.887135,
                    y: 40.764907,
                })
        {
            println!(
                "PARALLEL DEBUG: shorter->longer angle_diff={:.1}°, threshold={:.1}°",
                angle_diff, max_angle_diff
            );
        }

        if angle_diff <= max_angle_diff {
            return true;
        }
    }

    // Also try the reverse direction for robustness
    if let Some(angle_diff) = project_and_compare(longer, shorter) {
        // Debug output for the problematic case
        if shorter.0.first()
            == Some(&geo::Coord {
                x: -111.887135,
                y: 40.764907,
            })
            || longer.0.first()
                == Some(&geo::Coord {
                    x: -111.887135,
                    y: 40.764907,
                })
        {
            println!(
                "PARALLEL DEBUG: longer->shorter angle_diff={:.1}°, threshold={:.1}°",
                angle_diff, max_angle_diff
            );
        }

        if angle_diff <= max_angle_diff {
            return true;
        }
    }

    false
}

/// Normalize angle difference to smallest absolute value (0 to 180 degrees).
fn normalize_angle_diff(angle_diff: f64) -> f64 {
    let mut diff = angle_diff.abs();
    if diff > 180.0 {
        diff = 360.0 - diff;
    }
    diff
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
                let crosses = ways_cross_geometrically(target_way, &candidate.way, data);
                let is_parallel = are_ways_geometrically_parallel(
                    &target_linestring,
                    &candidate.linestring,
                    max_angle_diff,
                );

                // Debug output for the problematic case
                if target_way.id == 10128331 && candidate.way.id == 708994242 {
                    println!(
                        "DEBUG: way {} vs way {}: crosses={}, is_parallel={}",
                        target_way.id, candidate.way.id, crosses, is_parallel
                    );
                }

                // Exclude ways that cross geometrically (perpendicular crossings)
                !crosses && is_parallel
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

        let sample_count = (Geodesic.length(&target_linestring) / 10.0)
            .ceil()
            .max(10.0) as usize;
        let mut covered_samples = 0;

        for i in 0..sample_count {
            let t = i as f64 / (sample_count - 1) as f64;
            let sample_point = target_linestring
                .point_at_ratio_from_start(&Euclidean, t)
                .unwrap();

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

/// Calculate the minimum distance from a point to a LineString using proper point-to-segment distance.
pub fn calculate_point_to_linestring_distance(
    point: &geo::Point,
    linestring: &geo::LineString,
) -> f64 {
    use geo::ClosestPoint;

    linestring
        .lines()
        .map(|line| {
            match line.closest_point(point) {
                geo::Closest::SinglePoint(closest) | geo::Closest::Intersection(closest) => {
                    Haversine.distance(*point, closest)
                }
                geo::Closest::Indeterminate => {
                    // This should be extremely rare with geo crate's robust implementation
                    f64::INFINITY
                }
            }
        })
        .fold(f64::MAX, f64::min)
}

/// Calculate the minimum distance between two LineStrings.
/// Tests both directions to ensure true bidirectional distance.
pub fn calculate_linestring_to_linestring_distance(
    linestring1: &geo::LineString,
    linestring2: &geo::LineString,
) -> f64 {
    // Test distances from linestring1 points to linestring2
    let dist1_to_2 = linestring1
        .points()
        .map(|p1| calculate_point_to_linestring_distance(&p1, linestring2))
        .fold(f64::MAX, f64::min);

    // Test distances from linestring2 points to linestring1
    let dist2_to_1 = linestring2
        .points()
        .map(|p2| calculate_point_to_linestring_distance(&p2, linestring1))
        .fold(f64::MAX, f64::min);

    // Return the minimum distance found in either direction
    dist1_to_2.min(dist2_to_1)
}

/// Detects if two ways cross at a perpendicular or near-perpendicular angle.
/// This includes both geometric crossings and shared-node crossings.
/// Used to exclude perpendicular sidewalks from parallel coverage analysis.
pub fn ways_cross_geometrically(way1: &OsmWay, way2: &OsmWay, data: &OsmData) -> bool {
    let Some(linestring1) = way_to_linestring(way1, data) else {
        return false;
    };
    let Some(linestring2) = way_to_linestring(way2, data) else {
        return false;
    };

    let intersection_matrix = linestring1.relate(&linestring2);

    // Check for crosses (includes both geometric and topological crossings)
    if intersection_matrix.is_crosses() {
        return true;
    }

    // Also check for intersections where ways share endpoints but cross at steep angles
    if intersection_matrix.is_intersects() && way1.nodes.iter().any(|n| way2.nodes.contains(n)) {
        // If ways intersect and share nodes, check if they cross at a steep angle
        return are_ways_crossing_at_steep_angle(&linestring1, &linestring2, 45.0);
    }

    false
}

/// Checks if two linestrings intersect at a steep angle (indicating a crossing rather than parallel ways).
/// Returns true if the angle between the linestrings is greater than the threshold.
fn are_ways_crossing_at_steep_angle(
    linestring1: &LineString,
    linestring2: &LineString,
    min_angle_degrees: f64,
) -> bool {
    // Get the overall direction of each linestring
    let Some(dir1) = get_linestring_overall_direction(linestring1) else {
        return false;
    };
    let Some(dir2) = get_linestring_overall_direction(linestring2) else {
        return false;
    };

    // Calculate the angle difference between the two directions
    let angle_diff = normalize_angle_diff(dir1 - dir2);

    // Consider it a crossing if the angle is steep (> min_angle_degrees from parallel)
    // Parallel would be 0° or 180°, so check the angle from parallel
    let angle_from_parallel = angle_diff.min(180.0 - angle_diff);

    // It's a crossing if it's closer to perpendicular than to parallel by the threshold
    angle_from_parallel > min_angle_degrees
}

/// Get the overall direction (bearing) of a linestring from start to end.
fn get_linestring_overall_direction(linestring: &LineString) -> Option<f64> {
    if linestring.0.len() < 2 {
        return None;
    }

    let start_point = Point::from(linestring.0[0]);
    let end_point = Point::from(linestring.0[linestring.0.len() - 1]);

    if start_point == end_point {
        return None;
    }

    // Use the existing direction function
    let line = Line::new(linestring.0[0], linestring.0[linestring.0.len() - 1]);
    let geographic = is_geographic(linestring);
    direction(&line, geographic)
}

/// Find actual crossing points where two ways intersect geometrically.
/// Returns intersection points for ways that cross without sharing nodes.
pub fn find_way_crossing_points(way1: &OsmWay, way2: &OsmWay, data: &OsmData) -> Vec<Point> {
    // First check if they actually cross
    if !ways_cross_geometrically(way1, way2, data) {
        return Vec::new();
    }

    let Some(linestring1) = way_to_linestring(way1, data) else {
        return Vec::new();
    };
    let Some(linestring2) = way_to_linestring(way2, data) else {
        return Vec::new();
    };

    let mut crossings = Vec::new();

    // Find actual intersection points between line segments
    for line1 in linestring1.lines() {
        for line2 in linestring2.lines() {
            if let Some(intersection) = line_intersection(line1, line2) {
                match intersection {
                    LineIntersection::SinglePoint { intersection, .. } => {
                        crossings.push(Point::from(intersection));
                    }
                    LineIntersection::Collinear { intersection } => {
                        // For collinear segments, use start point of intersection
                        crossings.push(Point::from(intersection.start_point()));
                    }
                }
            }
        }
    }

    crossings
}

/// Analyze similarity between two ways using Fréchet distance.
/// Lower values indicate more similar (parallel) paths. Good for route comparison.
pub fn calculate_way_similarity(way1: &OsmWay, way2: &OsmWay, data: &OsmData) -> Option<f64> {
    let linestring1 = way_to_linestring(way1, data)?;
    let linestring2 = way_to_linestring(way2, data)?;

    // Use geo crate's Fréchet distance for path similarity
    Some(Euclidean.frechet_distance(&linestring1, &linestring2))
}

/// Enhanced parallel way detection using both bearing analysis and Fréchet distance.
/// Combines geometric parallelism with path similarity for robust detection.
pub fn are_ways_parallel(
    way1: &OsmWay,
    way2: &OsmWay,
    data: &OsmData,
    max_angle_diff: f64,
    max_frechet_distance: f64,
) -> bool {
    let Some(linestring1) = way_to_linestring(way1, data) else {
        return false;
    };
    let Some(linestring2) = way_to_linestring(way2, data) else {
        return false;
    };

    // Check geometric parallelism first (faster)
    if !are_ways_geometrically_parallel(&linestring1, &linestring2, max_angle_diff) {
        return false;
    }

    // Then check path similarity using Fréchet distance
    let frechet_dist = Euclidean.frechet_distance(&linestring1, &linestring2);
    frechet_dist <= max_frechet_distance
}

/// Convert an OSM way to a LineString geometry using optimized coordinate processing.
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

/// Calculate bounding box efficiently using CoordsIter for large geometries.
pub fn calculate_efficient_bounding_box(linestring: &LineString) -> Option<geo::Rect> {
    let mut coords_iter = linestring.coords_iter();
    let first_coord = coords_iter.next()?;

    let mut min_x = first_coord.x;
    let mut max_x = first_coord.x;
    let mut min_y = first_coord.y;
    let mut max_y = first_coord.y;

    // Process remaining coordinates efficiently
    for coord in coords_iter {
        min_x = min_x.min(coord.x);
        max_x = max_x.max(coord.x);
        min_y = min_y.min(coord.y);
        max_y = max_y.max(coord.y);
    }

    Some(geo::Rect::new(
        Coord { x: min_x, y: min_y },
        Coord { x: max_x, y: max_y },
    ))
}

/// Efficiently count coordinates in a geometry without collecting.
pub fn count_coordinates(linestring: &LineString) -> usize {
    linestring.coords_iter().count()
}

/// Process coordinates efficiently for distance calculations.
pub fn calculate_efficient_linestring_distance(
    linestring1: &LineString,
    linestring2: &LineString,
) -> f64 {
    // Use CoordsIter to avoid collecting all points at once
    let min_dist1_to_2 = linestring1
        .coords_iter()
        .map(|coord| {
            let point = Point::from(coord);
            calculate_point_to_linestring_distance(&point, linestring2)
        })
        .fold(f64::MAX, f64::min);

    let min_dist2_to_1 = linestring2
        .coords_iter()
        .map(|coord| {
            let point = Point::from(coord);
            calculate_point_to_linestring_distance(&point, linestring1)
        })
        .fold(f64::MAX, f64::min);

    min_dist1_to_2.min(min_dist2_to_1)
}

/// Validate OSM way geometry using geo crate validation.
/// Returns true if the geometry is valid for spatial operations.
pub fn validate_way_geometry(way: &OsmWay, data: &OsmData) -> bool {
    let Some(linestring) = way_to_linestring(way, data) else {
        return false;
    };

    // Check basic validity requirements
    if linestring.coords_iter().count() < 2 {
        return false;
    }

    // Use geo crate's validation if available, otherwise basic checks
    linestring.is_valid()
}

/// Comprehensive spatial quality check for OSM ways.
/// Checks for common issues that affect spatial analysis accuracy.
#[derive(Debug, Clone)]
pub struct WayQualityReport {
    pub is_valid: bool,
    pub coordinate_count: usize,
    pub length_meters: f64,
    pub has_duplicate_points: bool,
    pub has_self_intersection: bool,
}

pub fn analyze_way_quality(way: &OsmWay, data: &OsmData) -> Option<WayQualityReport> {
    let linestring = way_to_linestring(way, data)?;
    let coord_count = count_coordinates(&linestring);

    // Check for duplicate consecutive points
    let has_duplicates = linestring
        .lines()
        .any(|line| line.start_point() == line.end_point());

    // Check for self-intersection (basic check)
    let has_self_intersection = check_self_intersection(&linestring);

    Some(WayQualityReport {
        is_valid: validate_way_geometry(way, data),
        coordinate_count: coord_count,
        length_meters: Geodesic.length(&linestring),
        has_duplicate_points: has_duplicates,
        has_self_intersection,
    })
}

/// Basic self-intersection check for LineStrings.
fn check_self_intersection(linestring: &LineString) -> bool {
    let lines: Vec<_> = linestring.lines().collect();

    // Check each pair of non-adjacent line segments
    for (i, line1) in lines.iter().enumerate() {
        for (j, line2) in lines.iter().enumerate() {
            // Skip adjacent segments
            if (i as i32 - j as i32).abs() <= 1 {
                continue;
            }

            if line_intersection(*line1, *line2).is_some() {
                return true;
            }
        }
    }

    false
}
