//! Coordinate system transformations
//!
//! Utilities for transforming between different coordinate reference systems
//! following GeoRust best practices

use geo::*;
use proj::Proj;

/// Transform a point from WGS84 to Web Mercator (EPSG:3857)
///
/// Commonly used for accurate distance calculations in meters
///
/// # Arguments
/// * `point` - Point in WGS84 coordinates (longitude, latitude)
///
/// # Returns
/// Point in Web Mercator coordinates, panics on error
pub fn point_wgs84_to_web_mercator(point: Point) -> Point {
    let transform = Proj::new_known_crs("EPSG:4326", "EPSG:3857", None)
        .expect("Failed to create WGS84 to Web Mercator transformation");
    transform.convert(point).expect("Failed to transform point")
}

/// Transform a point from Web Mercator to WGS84
///
/// # Arguments
/// * `point` - Point in Web Mercator coordinates
///
/// # Returns
/// Point in WGS84 coordinates (longitude, latitude)
pub fn point_web_mercator_to_wgs84(point: Point) -> Point {
    let transform = Proj::new_known_crs("EPSG:3857", "EPSG:4326", None)
        .expect("Failed to create Web Mercator to WGS84 transformation");
    transform.convert(point).expect("Failed to transform point")
}

/// Get appropriate local UTM zone for a given WGS84 coordinate
///
/// UTM provides accurate distance calculations for local areas
///
/// # Arguments
/// * `point` - Point in WGS84 coordinates
///
/// # Returns
/// UTM EPSG code as string (e.g., "EPSG:32633" for UTM Zone 33N)
pub fn get_local_utm_crs(point: Point) -> String {
    let lon = point.x();
    let lat = point.y();
    
    // Calculate UTM zone number
    let zone = ((lon + 180.0) / 6.0).floor() as i32 + 1;
    
    // Determine hemisphere (N or S)
    let hemisphere = if lat >= 0.0 { "326" } else { "327" }; // 326xx for North, 327xx for South
    
    format!("EPSG:{}{:02}", hemisphere, zone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    
    #[test]
    fn test_point_wgs84_to_web_mercator() {
        let berlin = Point::new(13.4050, 52.5200);
        let mercator = point_wgs84_to_web_mercator(berlin);
        
        // Web Mercator coordinates should be much larger
        assert!(mercator.x().abs() > 1_000_000.0);
        assert!(mercator.y().abs() > 1_000_000.0);
    }
    
    #[test]
    fn test_point_roundtrip_transformation() {
        let original = Point::new(13.4050, 52.5200);
        
        let mercator = point_wgs84_to_web_mercator(original);
        let back_to_wgs84 = point_web_mercator_to_wgs84(mercator);
        
        // Should be very close to original (within projection precision)
        assert_relative_eq!(back_to_wgs84.x(), original.x(), epsilon = 1e-6);
        assert_relative_eq!(back_to_wgs84.y(), original.y(), epsilon = 1e-6);
    }
    
    #[test]
    fn test_get_local_utm_crs() {
        // Berlin should be in UTM Zone 33N
        let berlin = Point::new(13.4050, 52.5200);
        let utm_crs = get_local_utm_crs(berlin);
        assert_eq!(utm_crs, "EPSG:32633");
        
        // Southern hemisphere test
        let sydney = Point::new(151.2093, -33.8688);
        let utm_crs_sydney = get_local_utm_crs(sydney);
        // Should be Zone 56, Southern hemisphere
        assert_eq!(utm_crs_sydney, "EPSG:32756");
    }
}
