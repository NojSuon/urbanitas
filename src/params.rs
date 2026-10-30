//! The knobs that describe a building before it is generated.

use std::fmt;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const MAX_FLOORS: u32 = 12;
pub const MIN_SIDE: f64 = 5.0;
pub const MAX_SIDE: f64 = 60.0;
pub const MAX_ROOMS: u32 = 24;
/// Smallest footprint area, in m², allowed per room.
pub const MIN_ROOM_AREA: f64 = 12.0;
/// Width of the stair core that multi-storey buildings reserve on their west side.
pub const STAIR_WIDTH: f64 = 2.6;
/// Length of a straight stair flight, along the depth of the building.
pub const STAIR_LENGTH: f64 = 4.8;
/// A hall in front of the stairs is only carved out when it gets at least this depth.
pub const MIN_HALL_DEPTH: f64 = 2.0;
/// Narrowest room strip left beside the stair core.
pub const MIN_USABLE_WIDTH: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum BuildingType {
    /// A family home: living spaces downstairs, bedrooms upstairs.
    House,
    /// A larger home with a lounge, guest rooms and usually a pool.
    Villa,
    /// The same flat repeated on every floor, with balconies.
    Apartment,
    /// A shop on the ground floor with a flat above it.
    Shop,
}

impl BuildingType {
    pub fn label(self) -> &'static str {
        match self {
            BuildingType::House => "house",
            BuildingType::Villa => "villa",
            BuildingType::Apartment => "apartment",
            BuildingType::Shop => "shop",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Garden {
    /// No garden: the building sits on a paved plinth.
    None,
    /// A front garden between the street and the entrance.
    Front,
    /// A back garden with a terrace, often a pergola or a pool.
    Back,
    /// Garden on every side.
    Surrounding,
}

impl Garden {
    pub fn label(self) -> &'static str {
        match self {
            Garden::None => "none",
            Garden::Front => "front",
            Garden::Back => "back",
            Garden::Surrounding => "surrounding",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureStyle {
    /// Whitewashed thick walls, arched openings, shutters, terracotta or flat terrace roofs.
    Mediterranean,
}

impl ArchitectureStyle {
    pub fn label(self) -> &'static str {
        match self {
            ArchitectureStyle::Mediterranean => "Mediterranean",
        }
    }
}

/// Footprint size in metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Size {
    /// West → east.
    pub width: f64,
    /// South (front) → north (back).
    pub depth: f64,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            width: 12.0,
            depth: 10.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildingParams {
    pub seed: u64,
    pub floors: u32,
    pub size: Size,
    pub building_type: BuildingType,
    /// Rooms per floor, not counting the hall and stairs.
    pub rooms: u32,
    pub garden: Garden,
    pub style: ArchitectureStyle,
}

impl Default for BuildingParams {
    fn default() -> Self {
        Self {
            seed: 1,
            floors: 2,
            size: Size::default(),
            building_type: BuildingType::House,
            rooms: 4,
            garden: Garden::Front,
            style: ArchitectureStyle::Mediterranean,
        }
    }
}

impl BuildingParams {
    pub fn validate(&self) -> Result<(), ParamError> {
        if !(1..=MAX_FLOORS).contains(&self.floors) {
            return Err(ParamError::Floors(self.floors));
        }
        for (dimension, value) in [("width", self.size.width), ("depth", self.size.depth)] {
            if !value.is_finite() || !(MIN_SIDE..=MAX_SIDE).contains(&value) {
                return Err(ParamError::Size { dimension, value });
            }
        }
        if !(1..=MAX_ROOMS).contains(&self.rooms) {
            return Err(ParamError::Rooms(self.rooms));
        }
        let usable_width = if self.floors > 1 {
            self.size.width - STAIR_WIDTH
        } else {
            self.size.width
        };
        if usable_width < MIN_USABLE_WIDTH {
            return Err(ParamError::NoRoomForStairs {
                width: self.size.width,
            });
        }
        let area_per_room = usable_width * self.size.depth / self.rooms as f64;
        if area_per_room < MIN_ROOM_AREA {
            return Err(ParamError::TooCrowded {
                rooms: self.rooms,
                area_per_room,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParamError {
    Floors(u32),
    Size { dimension: &'static str, value: f64 },
    Rooms(u32),
    NoRoomForStairs { width: f64 },
    TooCrowded { rooms: u32, area_per_room: f64 },
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::Floors(n) => {
                write!(f, "floors must be between 1 and {MAX_FLOORS}, got {n}")
            }
            ParamError::Size { dimension, value } => write!(
                f,
                "{dimension} must be between {MIN_SIDE} and {MAX_SIDE} metres, got {value}"
            ),
            ParamError::Rooms(n) => write!(f, "rooms must be between 1 and {MAX_ROOMS}, got {n}"),
            ParamError::NoRoomForStairs { width } => write!(
                f,
                "a {width} m wide building has no room for stairs; multi-storey buildings need at least {} m",
                STAIR_WIDTH + MIN_USABLE_WIDTH
            ),
            ParamError::TooCrowded {
                rooms,
                area_per_room,
            } => write!(
                f,
                "{rooms} rooms per floor leaves {area_per_room:.1} m² each; use fewer rooms or a larger size (at least {MIN_ROOM_AREA} m² per room)"
            ),
        }
    }
}

impl std::error::Error for ParamError {}
