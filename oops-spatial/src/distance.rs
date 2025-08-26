//! Distance calculation utilities

use geo::*;

/// Calculate the Euclidean distance between two points in degrees
///
/// # Arguments
/// * `p1` - First point
/// * `p2` - Second point
///
/// # Returns
/// Distance in degrees (not meters - use `distance_meters` for that)
pub fn distance_degrees(p1: Point, p2: Point) -> f64 {
    p1.euclidean_distance(&p2)
}

/// Calculate approximate distance between two points in meters
///
/// Uses the haversine formula for more accurate distances over the Earth's surface.
/// This is more accurate than degree-based calculations for geospatial analysis.
///
/// # Arguments
/// * `p1` - First point (longitude, latitude in degrees)
/// * `p2` - Second point (longitude, latitude in degrees)
///
/// # Returns
/// Distance in meters
pub fn distance_meters(p1: Point, p2: Point) -> f64 {
    p1.haversine_distance(&p2)
}

/// Calculate the shortest distance from a point to a linestring in meters
///
/// # Arguments
/// * `point` - The point
/// * `line` - The linestring
///
/// # Returns
/// Shortest distance in meters
pub fn point_to_line_distance_meters(point: Point, line: &LineString) -> f64 {
    if line.coords().count() == 0 {
        return f64::INFINITY;
    }
    
    let mut min_distance = f64::INFINITY;
    
    // Check distance to each segment of the linestring
    for segment in line.lines() {
        let distance = point_to_line_segment_distance_meters(point, segment);
        if distance < min_distance {
            min_distance = distance;
        }
    }
    
    min_distance
}

/// Calculate distance from a point to a line segment in meters
///
/// # Arguments
/// * `point` - The point
/// * `segment` - The line segment
///
/// # Returns
/// Distance in meters
pub fn point_to_line_segment_distance_meters(point: Point, segment: Line) -> f64 {
    // Project point onto the line segment
    let start = segment.start_point();
    let end = segment.end_point();
    
    // Vector from start to end
    let dx = end.x() - start.x();
    let dy = end.y() - start.y();
    
    // If the segment has zero length, return distance to start point
    if dx.abs() < f64::EPSILON && dy.abs() < f64::EPSILON {
        return distance_meters(point, start);
    }
    
    // Calculate projection parameter t
    let t = ((point.x() - start.x()) * dx + (point.y() - start.y()) * dy) / (dx * dx + dy * dy);
    
    let closest_point = if t < 0.0 {
        // Closest point is the start of the segment
        start
    } else if t > 1.0 {
        // Closest point is the end of the segment
        end
    } else {
        // Closest point is on the segment
        Point::new(start.x() + t * dx, start.y() + t * dy)
    };
    
    distance_meters(point, closest_point)
}

/// Find all points within a given distance of a target point
///
/// # Arguments
/// * `target` - The center point
/// * `candidates` - Vector of candidate points
/// * `max_distance_meters` - Maximum distance in meters
///
/// # Returns
/// Vector of points within the distance threshold
pub fn points_within_distance(target: Point, candidates: &[Point], max_distance_meters: f64) -> Vec<Point> {
    candidates
        .iter()
        .filter(|&point| distance_meters(target, *point) <= max_distance_meters)
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    
    #[test]
    fn test_distance_degrees_same_point() {
        let point = Point::new(13.4050, 52.5200);
        let distance = distance_degrees(point, point);
        assert_relative_eq!(distance, 0.0, epsilon = 1e-10);
    }
    
    #[test]
    fn test_distance_meters_same_point() {
        let point = Point::new(13.4050, 52.5200);
        let distance = distance_meters(point, point);
        assert_relative_eq!(distance, 0.0, epsilon = 1e-3);
    }
    
    #[test]
    fn test_distance_meters_known_distance() {
        // Berlin to Munich - approximately 504 km
        let berlin = Point::new(13.4050, 52.5200);
        let munich = Point::new(11.5820, 48.1351);
        
        let distance = distance_meters(berlin, munich);
        
        // Should be approximately 504,000 meters (allow 5% tolerance)
        assert!(distance > 480_000.0 && distance < 530_000.0, 
                "Distance was {}, expected ~504,000m", distance);
    }
    
    #[test]
    fn test_distance_meters_short_distance() {
        // Two points ~100m apart in Berlin
        let p1 = Point::new(13.4050, 52.5200);
        let p2 = Point::new(13.4060, 52.5200); // ~0.001 degrees longitude ≈ 67m at Berlin latitude
        
        let distance = distance_meters(p1, p2);
        
        // Should be approximately 67 meters (allow 20% tolerance for approximation)
        assert!(distance > 50.0 && distance < 85.0,
                "Distance was {}m, expected ~67m", distance);
    }
    
    #[test]
    fn test_point_to_line_distance_simple() {
        let point = Point::new(0.0, 1.0);
        let line = LineString::from(vec![(0.0, 0.0), (2.0, 0.0)]); // horizontal line
        
        let distance = point_to_line_distance_meters(point, &line);
        
        // Should be approximately 111,320 meters (1 degree latitude)
        assert!(distance > 100_000.0 && distance < 120_000.0,
                "Distance was {}m", distance);
    }
    
    #[test]
    fn test_point_to_line_distance_on_line() {
        let point = Point::new(1.0, 0.0);
        let line = LineString::from(vec![(0.0, 0.0), (2.0, 0.0)]);
        
        let distance = point_to_line_distance_meters(point, &line);
        
        // Point is on the line, distance should be very small
        assert!(distance < 1.0, "Distance should be ~0, was {}m", distance);
    }
    
    #[test]
    fn test_point_to_line_distance_empty_line() {
        let point = Point::new(0.0, 0.0);
        let empty_line = LineString::new(vec![]);
        
        let distance = point_to_line_distance_meters(point, &empty_line);
        
        assert_eq!(distance, f64::INFINITY);
    }
    
    #[test]
    fn test_point_to_line_segment_distance() {
        let point = Point::new(1.0, 1.0);
        let segment = Line::from([(0.0, 0.0), (2.0, 0.0)]);
        
        let distance = point_to_line_segment_distance_meters(point, segment);
        
        // Distance from (1,1) to line y=0 should be ~111,320m (1 degree latitude)
        assert!(distance > 100_000.0 && distance < 120_000.0);
    }
    
    #[test]
    fn test_point_to_line_segment_at_endpoint() {
        let point = Point::new(0.0, 0.0);
        let segment = Line::from([(0.0, 0.0), (1.0, 0.0)]);
        
        let distance = point_to_line_segment_distance_meters(point, segment);
        
        // Point is at the start of the segment
        assert!(distance < 1.0);
    }
    
    #[test]
    fn test_points_within_distance() {
        let center = Point::new(0.0, 0.0);
        let candidates = vec![
            Point::new(0.0, 0.0001), // ~11m north
            Point::new(0.0, 0.001),  // ~111m north  
            Point::new(0.0, 0.01),   // ~1.1km north
        ];
        
        let nearby = points_within_distance(center, &candidates, 100.0);
        
        // Should find only the first point (11m away)
        assert_eq!(nearby.len(), 1);
        assert_relative_eq!(nearby[0].y(), 0.0001, epsilon = 1e-10);
    }
    
    #[test]
    fn test_points_within_distance_none() {
        let center = Point::new(0.0, 0.0);
        let candidates = vec![
            Point::new(0.0, 0.01),   // ~1.1km north
            Point::new(0.0, 0.02),   // ~2.2km north
        ];
        
        let nearby = points_within_distance(center, &candidates, 100.0);
        
        // Should find no points within 100m
        assert_eq!(nearby.len(), 0);
    }
    
    #[test]
    fn test_points_within_distance_all() {
        let center = Point::new(0.0, 0.0);
        let candidates = vec![
            Point::new(0.0, 0.0001), // ~11m north
            Point::new(0.0, 0.0002), // ~22m north
        ];
        
        let nearby = points_within_distance(center, &candidates, 1000.0);
        
        // Should find all points within 1km
        assert_eq!(nearby.len(), 2);
    }
}