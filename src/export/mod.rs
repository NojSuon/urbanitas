//! Output formats for a generated [`Building`](crate::Building).
//!
//! - [`obj`]: a Wavefront OBJ mesh with an MTL material library, for Blender,
//!   game engines and other 3D tools.
//! - [`svg`]: one architectural floor plan per floor.
//! - [`json`]: the full building model.

pub mod json;
pub mod obj;
pub mod svg;
