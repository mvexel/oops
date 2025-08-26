//! Generic buffer operations for geometric primitives
//!
//! Following GeoRust best practices:
//! - Always work in projected coordinates for accurate distance calculations
//! - Use proper coordinate reference systems
//! - Leverage geo-types traits for clean implementations

use geo::*;
use proj::Proj;

/// Create a circular buffer around a point in WGS84 coordinates
/// 
/// 1. Projects the point to Web Mercator for accurate distance calculations
/// 2. Creates the buffer in projected space
/// 3. Projects back to WGS84 for consistent coordinate system
/// 
/// # Arguments
/// * `point` - The center point in WGS84 (longitude, latitude)
/// * `distance_meters` - Buffer radius in meters
/// 
/// # Returns
/// A polygon representing the buffered area in WGS84 coordinates
/// 
/// # Panics
/// Panics if coordinate transformation fails
pub fn buffer_point_wgs84(point: Point, distance_meters: f64) -> Polygon {
    buffer_point_projected(point, distance_meters, "EPSG:3857") // Web Mercator
}

/// Create a circular buffer around a point with custom projection
/// 
/// # Arguments
/// * `point` - The center point in WGS84 (longitude, latitude)
/// * `distance_meters` - Buffer radius in meters
/// * `target_crs` - Target coordinate reference system for calculations
/// 
/// # Returns
/// A polygon representing the buffered area in WGS84 coordinates
pub fn buffer_point_projected(point: Point, distance_meters: f64, target_crs: &str) -> Polygon {
    // Set up coordinate transformations
    let to_projected = Proj::new_known_crs("EPSG:4326", target_crs, None)
        .expect("Failed to create WGS84 to projected transformation");
    let to_wgs84 = Proj::new_known_crs(target_crs, "EPSG:4326", None)
        .expect("Failed to create projected to WGS84 transformation");
    
    // Transform point to projected coordinates
    let projected_point = to_projected.convert(point)
        .expect("Failed to transform point to projected coordinates");
    
    // Create buffer in projected space (where meters are meaningful)
    let buffer_projected = create_circular_buffer(projected_point, distance_meters, 32);
    
    // Transform buffer back to WGS84
    let exterior_wgs84: Vec<Coord> = buffer_projected.exterior().coords()
        .map(|coord| {
            to_wgs84.convert(*coord)
                .expect("Failed to transform buffer coordinates back to WGS84")
        })
        .collect();
    
    Polygon::new(LineString::from(exterior_wgs84), vec![])
}

/// Create a circular buffer in projected coordinate space
/// 
/// # Arguments
/// * `center` - Center point in projected coordinates
/// * `radius_meters` - Buffer radius in meters
/// * `num_segments` - Number of segments for the circle approximation
/// 
/// # Returns
/// A polygon representing the circular buffer
fn create_circular_buffer(center: Point, radius_meters: f64, num_segments: usize) -> Polygon {
    let mut coords = Vec::with_capacity(num_segments + 1);
    
    for i in 0..num_segments {
        let angle = (i as f64) * 2.0 * std::f64::consts::PI / (num_segments as f64);
        let x = center.x() + radius_meters * angle.cos();
        let y = center.y() + radius_meters * angle.sin();
        coords.push(Coord { x, y });
    }
    
    // Close the polygon
    coords.push(coords[0]);
    
    Polygon::new(LineString::from(coords), vec![])
}

/// Simplified buffer for testing - uses degree approximation
/// 
/// **WARNING**: This is for testing only and produces inaccurate results.
/// Use `buffer_point_wgs84` for production code.
pub fn buffer_point_degrees_approx(point: Point, distance_meters: f64) -> Polygon {
    // Rough approximation for testing - not accurate!
    let degrees_per_meter = 1.0 / 111_320.0; 
    let radius_degrees = distance_meters * degrees_per_meter;
    
    create_circular_buffer(point, radius_degrees, 16)
}

/// Create a buffer around a linestring in WGS84 coordinates
///
/// Uses proper coordinate projection for accurate distance calculations.
///
/// # Arguments
/// * `line` - The linestring in WGS84 coordinates
/// * `distance_meters` - Buffer width in meters (half-width on each side)
///
/// # Returns
/// A polygon representing the buffered area in WGS84 coordinates
pub fn buffer_linestring_wgs84(line: &LineString, distance_meters: f64) -> Polygon {
    if line.coords().count() == 0 {
        return Polygon::new(LineString::new(vec![]), vec![]);
    }
    
    // Set up coordinate transformations to Web Mercator
    let to_projected = Proj::new_known_crs("EPSG:4326", "EPSG:3857", None)
        .expect("Failed to create WGS84 to Web Mercator transformation");
    let to_wgs84 = Proj::new_known_crs("EPSG:3857", "EPSG:4326", None)
        .expect("Failed to create Web Mercator to WGS84 transformation");
    
    // Transform linestring to projected coordinates
    let projected_coords: Vec<Coord> = line.coords()
        .map(|coord| {
            to_projected.convert(*coord)
                .expect("Failed to transform linestring coordinates")
        })
        .collect();
    
    let projected_line = LineString::from(projected_coords);
    
    // Create buffer in projected space using simplified approach
    // In production, you'd use a proper parallel curve algorithm
    let buffer_projected = create_linestring_buffer_simple(&projected_line, distance_meters);
    
    // Transform buffer back to WGS84
    let exterior_wgs84: Vec<Coord> = buffer_projected.exterior().coords()
        .map(|coord| {
            to_wgs84.convert(*coord)
                .expect("Failed to transform buffer back to WGS84")
        })
        .collect();
    
    Polygon::new(LineString::from(exterior_wgs84), vec![])
}

/// Create a simple buffer around a linestring in projected space
/// 
/// This is a simplified implementation that creates a rectangular buffer.
/// For production use, consider using a proper parallel curve algorithm.
fn create_linestring_buffer_simple(line: &LineString, distance_meters: f64) -> Polygon {
    // Calculate bounding box and expand by buffer distance
    let bbox = line.bounding_rect().expect("LineString should have valid bounds");
    
    let min_x = bbox.min().x - distance_meters;
    let max_x = bbox.max().x + distance_meters;
    let min_y = bbox.min().y - distance_meters;
    let max_y = bbox.max().y + distance_meters;
    
    let rect_coords = vec![
        Coord { x: min_x, y: min_y },
        Coord { x: max_x, y: min_y },
        Coord { x: max_x, y: max_y },
        Coord { x: min_x, y: max_y },
        Coord { x: min_x, y: min_y }, // close
    ];
    
    Polygon::new(LineString::from(rect_coords), vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    
    #[test]
    fn test_buffer_point_basic() {
        let point = Point::new(0.0, 0.0);
        let buffer = buffer_point_degrees_approx(point, 100.0);
        
        // Should have 17 coordinates (16 + 1 to close)
        assert_eq!(buffer.exterior().coords().count(), 17);
        
        // Should be centered at origin
        let centroid = buffer.centroid().unwrap();
        assert_relative_eq!(centroid.x(), 0.0, epsilon = 1e-10);
        assert_relative_eq!(centroid.y(), 0.0, epsilon = 1e-10);
    }
    
    #[test]
    fn test_buffer_point_radius() {
        let point = Point::new(13.4050, 52.5200); // Berlin
        let buffer = buffer_point_degrees_approx(point, 50.0); // 50m buffer
        
        let exterior = buffer.exterior();
        let coords: Vec<Coord> = exterior.coords().copied().collect();
        
        // Check that all points are roughly the same distance from center
        let degrees_per_meter = 1.0 / 111_320.0;
        let expected_radius = 50.0 * degrees_per_meter;
        
        for coord in coords.iter().take(16) { // Skip the closing coordinate
            let distance = ((coord.x - point.x()).powi(2) + (coord.y - point.y()).powi(2)).sqrt();
            assert_relative_eq!(distance, expected_radius, epsilon = 1e-10);
        }
    }
    
    #[test]
    fn test_buffer_point_different_distances() {
        let point = Point::new(0.0, 0.0);
        
        let buffer_10m = buffer_point_degrees_approx(point, 10.0);
        let buffer_50m = buffer_point_degrees_approx(point, 50.0);
        
        // 50m buffer should be larger than 10m buffer
        assert!(buffer_50m.unsigned_area() > buffer_10m.unsigned_area());
    }
    
    #[test]
    fn test_buffer_linestring_basic() {
        let line = LineString::from(vec![
            (0.0, 0.0),
            (0.0, 0.001), // ~111m north
        ]);
        
        let buffer = buffer_linestring_wgs84(&line, 50.0);
        
        // Should create a polygon
        assert!(buffer.exterior().coords().count() >= 4);
        
        // Should have positive area
        assert!(buffer.unsigned_area() > 0.0);
    }
    
    #[test]
    fn test_buffer_linestring_contains_original() {
        let line = LineString::from(vec![
            (13.4000, 52.5100),
            (13.4010, 52.5110),
        ]);
        
        let buffer = buffer_linestring_wgs84(&line, 25.0);
        
        // All points on the original line should be inside the buffer
        for coord in line.coords() {
            let point = Point::from(*coord);
            assert!(buffer.contains(&point), "Buffer should contain original line points");
        }
    }
    
    #[test]
    fn test_empty_linestring() {
        let empty_line = LineString::new(vec![]);
        let buffer = buffer_linestring_wgs84(&empty_line, 10.0);
        
        // Should return empty polygon
        assert_eq!(buffer.exterior().coords().count(), 0);
    }
}