//! Architectural styles: proportions, openings and colours.
//!
//! Each [`ArchitectureStyle`] maps to a [`StyleProfile`]. Adding a style means
//! adding a variant and its profile and palettes here; the generator and
//! exporters only read the profile.

use serde::{Deserialize, Serialize};

use crate::params::ArchitectureStyle;
use crate::rng::Rng;

/// Size and placement of one kind of opening, in metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpeningSpec {
    pub width: f64,
    pub height: f64,
    /// Height of the bottom edge above the floor.
    pub sill: f64,
    /// Round-arched top: the top `width / 2` of the opening is a half circle.
    pub arched: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleProfile {
    pub style: ArchitectureStyle,
    pub floor_height: f64,
    pub slab_thickness: f64,
    pub wall_thickness: f64,
    pub partition_thickness: f64,
    pub window: OpeningSpec,
    /// Bathrooms, storage, halls and stairs.
    pub small_window: OpeningSpec,
    pub entrance: OpeningSpec,
    pub balcony_door: OpeningSpec,
    pub storefront: OpeningSpec,
    pub interior_door: OpeningSpec,
    /// Minimum solid wall between neighbouring windows.
    pub window_gap: f64,
    pub shutters: bool,
    pub balcony_chance: f64,
    pub balcony_depth: f64,
    /// Chance a house gets a flat terrace roof instead of a hipped tile roof.
    pub flat_roof_chance: f64,
    pub roof_pitch_deg: f64,
    pub eave_overhang: f64,
    pub parapet_height: f64,
}

impl StyleProfile {
    pub fn for_style(style: ArchitectureStyle) -> Self {
        match style {
            ArchitectureStyle::Mediterranean => Self {
                style,
                floor_height: 3.2,
                slab_thickness: 0.2,
                wall_thickness: 0.35,
                partition_thickness: 0.12,
                window: OpeningSpec {
                    width: 1.0,
                    height: 1.5,
                    sill: 0.9,
                    arched: true,
                },
                small_window: OpeningSpec {
                    width: 0.6,
                    height: 0.6,
                    sill: 1.5,
                    arched: false,
                },
                entrance: OpeningSpec {
                    width: 1.2,
                    height: 2.5,
                    sill: 0.0,
                    arched: true,
                },
                balcony_door: OpeningSpec {
                    width: 1.1,
                    height: 2.3,
                    sill: 0.0,
                    arched: true,
                },
                storefront: OpeningSpec {
                    width: 2.4,
                    height: 2.6,
                    sill: 0.0,
                    arched: true,
                },
                interior_door: OpeningSpec {
                    width: 0.9,
                    height: 2.2,
                    sill: 0.0,
                    arched: true,
                },
                window_gap: 1.4,
                shutters: true,
                balcony_chance: 0.5,
                balcony_depth: 1.0,
                flat_roof_chance: 0.5,
                roof_pitch_deg: 22.0,
                eave_overhang: 0.45,
                parapet_height: 0.9,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// Components in `0.0..=1.0`, as OBJ materials expect.
    pub fn rgb(self) -> [f64; 3] {
        [
            self.0 as f64 / 255.0,
            self.1 as f64 / 255.0,
            self.2 as f64 / 255.0,
        ]
    }
}

/// The colours picked for one building.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub name: String,
    pub walls: Color,
    /// Shutters and front doors.
    pub trim: Color,
    pub roof: Color,
    pub stone: Color,
}

impl Palette {
    pub fn choose(style: ArchitectureStyle, rng: &mut Rng) -> Self {
        match style {
            ArchitectureStyle::Mediterranean => {
                const WALLS: [(&str, Color); 4] = [
                    ("whitewash", Color(0xF5, 0xF1, 0xE8)),
                    ("sand", Color(0xE8, 0xD5, 0xB0)),
                    ("ochre", Color(0xD9, 0xA5, 0x5B)),
                    ("blush", Color(0xEB, 0xC9, 0xB4)),
                ];
                const TRIM: [(&str, Color); 3] = [
                    ("aegean blue", Color(0x1F, 0x5F, 0x99)),
                    ("olive green", Color(0x5B, 0x6B, 0x2F)),
                    ("teal", Color(0x2A, 0x7F, 0x7A)),
                ];
                let (wall_name, walls) = *rng.pick(&WALLS);
                let (trim_name, trim) = *rng.pick(&TRIM);
                Self {
                    name: format!("{wall_name} & {trim_name}"),
                    walls,
                    trim,
                    roof: Color(0xB8, 0x5C, 0x38),
                    stone: Color(0xC9, 0xB4, 0x8F),
                }
            }
        }
    }
}
