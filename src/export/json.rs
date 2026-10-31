//! JSON: the generated building model, or a layout as a project file.

use crate::model::Building;
use crate::parts::Layout;

pub fn to_json(building: &Building) -> String {
    serde_json::to_string_pretty(building).expect("a building always serialises to JSON")
}

/// A layout as a project file the app can open.
pub fn layout_to_json(layout: &Layout) -> String {
    layout.to_json()
}
