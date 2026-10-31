//! Output formats for a generated [`Building`](crate::Building).
//!
//! - [`obj`]: a Wavefront OBJ mesh with an MTL material library and PNG
//!   textures, for Blender, game engines and other 3D tools.
//! - [`svg`]: one architectural floor plan per floor.
//! - [`json`]: the building model, or a layout as an app project file.

pub mod json;
pub mod obj;
pub mod svg;
