//! The generated building: floors, rooms, walls, openings, roof and garden.
//!
//! Everything here is plain data, so it serialises straight to JSON and the
//! exporters can draw it without re-running the generator.

use serde::{Deserialize, Serialize};

use crate::geometry::{Edge, Rect, Segment};
use crate::params::{BuildingParams, Garden, Shape};
use crate::style::{Palette, StyleProfile};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomKind {
    Living,
    Lounge,
    Kitchen,
    Dining,
    Bedroom,
    Bathroom,
    Study,
    Hall,
    Stairs,
    Retail,
    Storage,
    Office,
}

impl RoomKind {
    pub fn label(self) -> &'static str {
        match self {
            RoomKind::Living => "Living",
            RoomKind::Lounge => "Lounge",
            RoomKind::Kitchen => "Kitchen",
            RoomKind::Dining => "Dining",
            RoomKind::Bedroom => "Bedroom",
            RoomKind::Bathroom => "Bathroom",
            RoomKind::Study => "Study",
            RoomKind::Hall => "Hall",
            RoomKind::Stairs => "Stairs",
            RoomKind::Retail => "Shop",
            RoomKind::Storage => "Storage",
            RoomKind::Office => "Office",
        }
    }

    /// Lower ranks get the larger rooms.
    pub fn size_rank(self) -> u8 {
        match self {
            RoomKind::Living | RoomKind::Retail => 0,
            RoomKind::Lounge => 1,
            RoomKind::Bedroom | RoomKind::Kitchen | RoomKind::Office => 2,
            RoomKind::Dining => 3,
            RoomKind::Study => 4,
            RoomKind::Storage => 5,
            RoomKind::Bathroom => 6,
            RoomKind::Hall | RoomKind::Stairs => 7,
        }
    }

    /// Relative floor area this kind of room asks for when a floor is divided up.
    pub fn area_weight(self) -> f64 {
        match self {
            RoomKind::Retail => 2.0,
            RoomKind::Living => 1.8,
            RoomKind::Lounge => 1.4,
            RoomKind::Bedroom | RoomKind::Kitchen => 1.1,
            RoomKind::Dining | RoomKind::Office => 1.0,
            RoomKind::Study => 0.8,
            RoomKind::Storage => 0.6,
            RoomKind::Bathroom => 0.55,
            RoomKind::Hall | RoomKind::Stairs => 1.0,
        }
    }

    pub fn is_circulation(self) -> bool {
        matches!(self, RoomKind::Hall | RoomKind::Stairs)
    }

    pub fn small_windows(self) -> bool {
        matches!(
            self,
            RoomKind::Bathroom | RoomKind::Storage | RoomKind::Hall | RoomKind::Stairs
        )
    }

    pub fn allows_balcony(self) -> bool {
        matches!(
            self,
            RoomKind::Bedroom | RoomKind::Living | RoomKind::Lounge | RoomKind::Study
        )
    }

    /// Rooms nobody should have to walk through to reach another room.
    pub fn is_dead_end(self) -> bool {
        matches!(self, RoomKind::Bathroom | RoomKind::Storage)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub id: usize,
    pub kind: RoomKind,
    pub rect: Rect,
    pub area: f64,
}

impl Room {
    pub fn new(id: usize, kind: RoomKind, rect: Rect) -> Self {
        Self {
            id,
            kind,
            rect,
            area: rect.area(),
        }
    }
}

/// A doorway in an interior wall. `center` is measured along the wall's axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Door {
    pub center: f64,
    pub width: f64,
    pub height: f64,
    pub arched: bool,
}

/// A partition between two rooms on the same floor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InteriorWall {
    pub segment: Segment,
    pub rooms: [usize; 2],
    pub door: Option<Door>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Window,
    /// A front door.
    Door,
    BalconyDoor,
    Storefront,
    /// An open, doorless passage between rooms.
    Archway,
    /// A garden gate between stone piers, in a garden wall or fence.
    Gate,
}

impl OpeningKind {
    pub fn label(self) -> &'static str {
        match self {
            OpeningKind::Window => "Window",
            OpeningKind::Door => "Front door",
            OpeningKind::BalconyDoor => "Balcony door",
            OpeningKind::Storefront => "Storefront",
            OpeningKind::Archway => "Archway",
            OpeningKind::Gate => "Gate",
        }
    }
}

/// An opening in an exterior wall. `center` is measured along its edge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub kind: OpeningKind,
    /// The piece of the outline the opening is in.
    pub edge: Edge,
    pub center: f64,
    pub width: f64,
    pub height: f64,
    pub sill: f64,
    pub arched: bool,
    pub room: usize,
}

impl Opening {
    pub fn start(&self) -> f64 {
        self.center - self.width / 2.0
    }

    pub fn end(&self) -> f64 {
        self.center + self.width / 2.0
    }

    pub fn is_glazed(&self) -> bool {
        !matches!(self.kind, OpeningKind::Door | OpeningKind::Archway)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Balcony {
    pub edge: Edge,
    pub center: f64,
    pub width: f64,
    pub depth: f64,
    pub room: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Floor {
    pub level: u32,
    /// Height of the floor surface's underside above the ground.
    pub elevation: f64,
    pub height: f64,
    /// The wings this floor covers (upper floors may drop some, leaving terraces).
    pub wings: Vec<Rect>,
    /// The outline of those wings.
    pub outline: Vec<Edge>,
    pub rooms: Vec<Room>,
    pub walls: Vec<InteriorWall>,
    pub openings: Vec<Opening>,
    pub balconies: Vec<Balcony>,
}

impl Floor {
    pub fn name(&self) -> String {
        if self.level == 0 {
            "Ground floor".to_string()
        } else {
            format!("Floor {}", self.level)
        }
    }

    pub fn find(&self, kind: RoomKind) -> Option<&Room> {
        self.rooms.iter().find(|r| r.kind == kind)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Roof {
    /// A walkable terrace behind a low parapet wall or a balustrade.
    Flat {
        parapet_height: f64,
        #[serde(default)]
        balustrade: bool,
    },
    /// Four tiled slopes meeting at a ridge.
    Hip {
        pitch_deg: f64,
        overhang: f64,
        ridge_height: f64,
    },
}

impl Roof {
    pub fn label(&self) -> &'static str {
        match self {
            Roof::Flat {
                balustrade: true, ..
            } => "roof terrace",
            Roof::Flat { .. } => "flat terrace",
            Roof::Hip { .. } => "hipped terracotta",
        }
    }

    /// How far the roof rises above the top floor's ceiling slab.
    pub fn rise(&self) -> f64 {
        match *self {
            Roof::Flat { parapet_height, .. } => parapet_height,
            Roof::Hip { ridge_height, .. } => ridge_height,
        }
    }

    pub fn hip(rect: &Rect, pitch_deg: f64, overhang: f64) -> Roof {
        let short = rect.w.min(rect.d) + 2.0 * overhang;
        Roof::Hip {
            pitch_deg,
            overhang,
            ridge_height: short / 2.0 * pitch_deg.to_radians().tan(),
        }
    }
}

/// A roof over one wing, sitting on top of `level - 1`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoofSpec {
    pub rect: Rect,
    pub level: u32,
    pub roof: Roof,
    /// Sides (south, north, west, east) that meet another roof or a taller wall, and so get no parapet.
    pub open: [bool; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureKind {
    Path,
    Terrace,
    Pool,
    Pergola,
    OliveTree,
    CypressTree,
    LemonTree,
    Pot,
    /// A columned, arcaded porch.
    Loggia,
    Fountain,
    /// A paved internal courtyard ringed by a portico.
    Courtyard,
    Well,
    /// Low clipped hedges in a geometric pattern.
    Parterre,
    /// A small rooftop loggia (altana).
    Altana,
}

impl FeatureKind {
    pub fn label(self) -> &'static str {
        match self {
            FeatureKind::Loggia => "loggia",
            FeatureKind::Fountain => "fountain",
            FeatureKind::Path => "path",
            FeatureKind::Terrace => "terrace",
            FeatureKind::Pool => "pool",
            FeatureKind::Pergola => "pergola",
            FeatureKind::OliveTree => "olive",
            FeatureKind::CypressTree => "cypress",
            FeatureKind::LemonTree => "lemon",
            FeatureKind::Pot => "pot",
            FeatureKind::Courtyard => "courtyard",
            FeatureKind::Well => "well",
            FeatureKind::Parterre => "parterre",
            FeatureKind::Altana => "altana",
        }
    }

    pub fn is_tree(self) -> bool {
        matches!(
            self,
            FeatureKind::OliveTree | FeatureKind::CypressTree | FeatureKind::LemonTree
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GardenFeature {
    pub kind: FeatureKind,
    /// Ground footprint; for trees, the canopy.
    pub rect: Rect,
    pub height: f64,
    /// The level the feature stands on (0 for the garden; the roof for an altana).
    #[serde(default)]
    pub level: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GardenPlan {
    pub kind: Garden,
    /// The plot of land, including the building footprint.
    pub lot: Rect,
    pub features: Vec<GardenFeature>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Building {
    pub name: String,
    pub params: BuildingParams,
    pub style: StyleProfile,
    pub palette: Palette,
    pub shape: Shape,
    /// The bounding box of the footprint.
    pub footprint: Rect,
    /// The rectangles the footprint is made of.
    pub wings: Vec<Rect>,
    /// The open internal courtyard, for courtyard palazzi.
    pub courtyard: Option<Rect>,
    pub floors: Vec<Floor>,
    pub roofs: Vec<RoofSpec>,
    pub garden: Option<GardenPlan>,
}

impl Building {
    /// Ground to the top of the highest roof.
    pub fn height(&self) -> f64 {
        self.roofs
            .iter()
            .map(|r| {
                r.level as f64 * self.style.floor_height + self.style.slab_thickness + r.roof.rise()
            })
            .fold(0.0, f64::max)
    }

    pub fn room_count(&self) -> usize {
        self.floors.iter().map(|f| f.rooms.len()).sum()
    }

    /// The stair well, shared by every floor of a multi-storey building.
    pub fn stairs(&self) -> Option<Rect> {
        self.floors.first()?.find(RoomKind::Stairs).map(|r| r.rect)
    }

    /// The main roof's style, for summaries.
    pub fn roof_label(&self) -> &'static str {
        self.roofs
            .iter()
            .max_by_key(|r| r.level)
            .map_or("no", |r| r.roof.label())
    }
}
