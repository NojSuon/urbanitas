//! The building model as pretty-printed JSON.

use crate::model::Building;

pub fn to_json(building: &Building) -> String {
    serde_json::to_string_pretty(building).expect("a building always serialises to JSON")
}
