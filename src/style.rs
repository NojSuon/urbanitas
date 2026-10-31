//! Architectural styles: proportions, openings, details and colours.
//!
//! Each [`ArchitectureStyle`] maps to a [`StyleProfile`]. Adding a style means
//! adding a variant and its profile and palettes here; the generator, the
//! mesh builder and the exporters only read the profile.

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

const fn spec(width: f64, height: f64, sill: f64, arched: bool) -> OpeningSpec {
    OpeningSpec {
        width,
        height,
        sill,
        arched,
    }
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
    /// Zero means apartments and shops get hipped roofs too.
    pub flat_roof_chance: f64,
    pub roof_pitch_deg: f64,
    pub eave_overhang: f64,
    pub parapet_height: f64,
    /// Ground-floor facade in rough, bevelled stone blocks (bugnato).
    pub rusticated_ground: bool,
    /// Stone bands along each floor line, and a stone plinth at the ground.
    pub string_courses: bool,
    /// How far the moulded cornice under the eaves projects; zero for none.
    pub cornice_depth: f64,
    /// Stone frames, arches and keystones around exterior openings.
    pub window_surrounds: bool,
    /// Wooden rafter ends visible under the eaves.
    pub exposed_rafters: bool,
    /// Turned stone balusters on balconies instead of wrought iron.
    pub stone_balustrade: bool,
    /// A columned loggia over the back terrace instead of a pergola.
    pub loggia: bool,
    /// A stone fountain in the front garden.
    pub fountain: bool,
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
                window: spec(1.0, 1.5, 0.9, true),
                small_window: spec(0.6, 0.6, 1.5, false),
                entrance: spec(1.2, 2.5, 0.0, true),
                balcony_door: spec(1.1, 2.3, 0.0, true),
                storefront: spec(2.4, 2.6, 0.0, true),
                interior_door: spec(0.9, 2.2, 0.0, true),
                window_gap: 1.4,
                shutters: true,
                balcony_chance: 0.5,
                balcony_depth: 1.0,
                flat_roof_chance: 0.5,
                roof_pitch_deg: 22.0,
                eave_overhang: 0.45,
                parapet_height: 0.9,
                rusticated_ground: false,
                string_courses: false,
                cornice_depth: 0.0,
                window_surrounds: false,
                exposed_rafters: false,
                stone_balustrade: false,
                loggia: false,
                fountain: false,
            },
            ArchitectureStyle::Florentine => Self {
                style,
                floor_height: 3.6,
                slab_thickness: 0.2,
                wall_thickness: 0.45,
                partition_thickness: 0.14,
                window: spec(1.1, 1.9, 1.0, true),
                small_window: spec(0.6, 0.8, 1.6, false),
                entrance: spec(1.4, 3.0, 0.0, true),
                balcony_door: spec(1.2, 2.6, 0.0, true),
                storefront: spec(2.6, 3.0, 0.0, true),
                interior_door: spec(1.0, 2.4, 0.0, true),
                window_gap: 1.8,
                shutters: true,
                balcony_chance: 0.25,
                balcony_depth: 0.9,
                flat_roof_chance: 0.0,
                roof_pitch_deg: 18.0,
                eave_overhang: 1.0,
                parapet_height: 0.9,
                rusticated_ground: true,
                string_courses: true,
                cornice_depth: 0.55,
                window_surrounds: true,
                exposed_rafters: true,
                stone_balustrade: true,
                loggia: true,
                fountain: true,
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
            ArchitectureStyle::Florentine => {
                const WALLS: [(&str, Color); 4] = [
                    ("giallo fiorentino", Color(0xE3, 0xB9, 0x6A)),
                    ("terra di siena", Color(0xC9, 0x8B, 0x5E)),
                    ("crema", Color(0xEF, 0xE3, 0xC8)),
                    ("rosa antico", Color(0xD9, 0xA8, 0x9A)),
                ];
                const TRIM: [(&str, Color); 3] = [
                    ("verde persiana", Color(0x3F, 0x5B, 0x3A)),
                    ("verde oliva", Color(0x55, 0x60, 0x35)),
                    ("noce", Color(0x6B, 0x4A, 0x2F)),
                ];
                let (wall_name, walls) = *rng.pick(&WALLS);
                let (trim_name, trim) = *rng.pick(&TRIM);
                Self {
                    name: format!("{wall_name} & {trim_name}"),
                    walls,
                    trim,
                    roof: Color(0xB0, 0x5A, 0x3A),
                    // Pietra serena, the grey sandstone of Florentine trim.
                    stone: Color(0x9A, 0x9C, 0x96),
                }
            }
        }
    }
}
