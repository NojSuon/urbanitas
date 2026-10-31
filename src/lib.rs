//! # Urbanitas
//!
//! A procedural building generator by Noj Suon.
//!
//! Describe a building with [`BuildingParams`] (floors, size, type, rooms per
//! floor, garden and architectural style), call [`generate`], and export the
//! resulting [`Building`] as an OBJ mesh, SVG floor plans or JSON through the
//! [`export`] module. The same parameters and seed always produce the same
//! building.
//!
//! ```
//! use urbanitas::{generate, BuildingParams};
//!
//! let building = generate(&BuildingParams::default()).unwrap();
//! let layout = building.to_layout();
//! let obj = urbanitas::export::obj::to_obj(&layout, "house.mtl", "house");
//! assert!(obj.obj.contains("usemtl plaster"));
//! ```

pub mod export;
pub mod generate;
pub mod geometry;
pub mod layout;
pub mod mesh;
pub mod model;
pub mod params;
pub mod parts;
pub mod rng;
pub mod style;
pub mod texture;

pub use generate::generate;
pub use model::Building;
pub use params::{ArchitectureStyle, BuildingParams, BuildingType, Garden, ParamError, Size};
pub use parts::{Layout, Part, PartKind};
