//! OOPS Spatial - Generic geospatial operations
//!
//! This crate provides analyzer-agnostic spatial operations that can be
//! used by any analyzer. Domain-specific logic (like highway buffer distances)
//! should be implemented in the specific analyzer crates.

pub mod buffer;
pub mod distance;
pub mod intersection;
pub mod transform;

pub use buffer::*;
pub use distance::*;
pub use intersection::*;
pub use transform::*;

#[cfg(test)]
mod tests {    
    #[test]
    fn test_spatial_crate_loads() {
        // Basic smoke test
        assert!(true);
    }
}