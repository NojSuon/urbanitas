//! The modular building model.
//!
//! A [`Layout`] is a flat list of [`Part`]s: walls, openings mounted on walls,
//! floors, stairs, roofs, garden areas and props. The generator's output is
//! converted into parts ([`crate::layout::to_layout`]), and the app adds, moves
//! and removes parts by hand, so generated and hand-built buildings are the
//! same data. A layout serialises to JSON and doubles as the project file.

use serde::{Deserialize, Serialize};

use crate::geometry::{EPS, Rect};
use crate::model::{OpeningKind, Roof, RoomKind};
use crate::style::{Palette, StyleProfile};

pub type PartId = u64;

/// A point on the ground plane: `[x, z]`.
pub type P2 = [f64; 2];

/// Bumped when the project file format changes incompatibly.
pub const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub id: PartId,
    /// Storey the part stands on; the ground floor is 0. Roofs sit on the level above the top floor.
    pub level: u32,
    /// Height of the ground the part's building (or the part itself) stands on.
    #[serde(default)]
    pub ground: f64,
    /// The building the part belongs to; parts of one building move and recolour together.
    #[serde(default)]
    pub group: Option<u32>,
    pub kind: PartKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum PartKind {
    Wall(Wall),
    Opening(OpeningPart),
    Balcony(BalconyPart),
    Floor(FloorPart),
    Stairs(StairsPart),
    Roof(RoofPart),
    Area(AreaPart),
    Prop(PropPart),
}

/// A straight wall between two points on its centre line.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wall {
    pub a: P2,
    pub b: P2,
    pub height: f64,
    pub thickness: f64,
    /// Facade walls get plaster outside, string courses and the style's details.
    pub exterior: bool,
    /// How far above its level's base the wall starts (upper facades start on top of the slab).
    #[serde(default)]
    pub base: f64,
    #[serde(default)]
    pub finish: WallFinish,
}

/// What kind of wall it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallFinish {
    /// Part of a building.
    #[default]
    Building,
    /// A plastered garden wall with stone coping.
    Garden,
    /// A rough dry-stone field wall.
    DryStone,
    /// Wrought-iron railings on a low stone plinth.
    Fence,
}

impl WallFinish {
    pub fn label(self) -> &'static str {
        match self {
            WallFinish::Building => "Wall",
            WallFinish::Garden => "Garden wall",
            WallFinish::DryStone => "Dry-stone wall",
            WallFinish::Fence => "Iron fence",
        }
    }

    pub fn default_height(self) -> f64 {
        match self {
            WallFinish::Building => 3.2,
            WallFinish::Garden => 2.2,
            WallFinish::DryStone => 1.1,
            WallFinish::Fence => 1.6,
        }
    }

    pub fn default_thickness(self) -> f64 {
        match self {
            WallFinish::Building => 0.35,
            WallFinish::Garden => 0.4,
            WallFinish::DryStone => 0.55,
            WallFinish::Fence => 0.3,
        }
    }
}

impl Wall {
    pub fn length(&self) -> f64 {
        dist(self.a, self.b)
    }

    /// Unit vector from `a` to `b`.
    pub fn dir(&self) -> P2 {
        let l = self.length().max(EPS);
        [(self.b[0] - self.a[0]) / l, (self.b[1] - self.a[1]) / l]
    }

    /// Distance along the wall of the point closest to `p` (clamped to the wall)
    /// and how far `p` is from the centre line there.
    pub fn closest(&self, p: P2) -> (f64, f64) {
        let d = self.dir();
        let along =
            ((p[0] - self.a[0]) * d[0] + (p[1] - self.a[1]) * d[1]).clamp(0.0, self.length());
        let q = [self.a[0] + d[0] * along, self.a[1] + d[1] * along];
        (along, dist(p, q))
    }

    pub fn point_at(&self, along: f64) -> P2 {
        let d = self.dir();
        [self.a[0] + d[0] * along, self.a[1] + d[1] * along]
    }
}

/// A window, door or archway, mounted on whichever wall runs through `at`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpeningPart {
    pub kind: OpeningKind,
    /// Centre of the opening on the host wall's centre line.
    pub at: P2,
    pub width: f64,
    pub height: f64,
    /// Bottom of the opening above the level's base (the underside of its floor slab).
    pub sill: f64,
    pub arched: bool,
    /// Unit vector pointing out of the building, towards the street side of the opening.
    pub facing: P2,
}

/// A balcony slab with a railing, hung on the outside of a wall.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BalconyPart {
    pub at: P2,
    pub width: f64,
    pub depth: f64,
    pub facing: P2,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloorPart {
    pub rect: Rect,
    pub room: Option<RoomKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Heading {
    North,
    East,
    South,
    West,
}

impl Heading {
    pub const ALL: [Heading; 4] = [Heading::North, Heading::East, Heading::South, Heading::West];

    /// Unit vector on the ground plane. North is `+z`, east is `+x`.
    pub fn vector(self) -> P2 {
        match self {
            Heading::North => [0.0, 1.0],
            Heading::East => [1.0, 0.0],
            Heading::South => [0.0, -1.0],
            Heading::West => [-1.0, 0.0],
        }
    }

    pub fn turned_right(self) -> Heading {
        match self {
            Heading::North => Heading::East,
            Heading::East => Heading::South,
            Heading::South => Heading::West,
            Heading::West => Heading::North,
        }
    }
}

/// A straight flight of steps up to the next level, climbing towards `heading`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StairsPart {
    pub rect: Rect,
    pub rise: f64,
    pub heading: Heading,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoofPart {
    /// The outside of the walls the roof covers; eaves overhang it.
    pub rect: Rect,
    pub roof: Roof,
    /// Sides (south, north, west, east) that meet another roof or a taller wall: no parapet there.
    #[serde(default)]
    pub open: [bool; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaKind {
    Lawn,
    Path,
    Terrace,
    Pool,
    Pergola,
    Loggia,
    Hedge,
    /// A paved internal courtyard ringed by an arcaded portico.
    Courtyard,
    /// Low box hedges in a geometric pattern.
    Parterre,
    /// A street paved with dark basalt cobbles.
    Street,
    /// A square paved with large stone slabs.
    Piazza,
    /// A white dirt road.
    Road,
    /// Rows of vines on posts.
    Vineyard,
    /// A belvedere tower.
    Tower,
    /// A level stone platform with retaining walls, for building on slopes.
    Platform,
}

impl AreaKind {
    pub const ALL: [AreaKind; 15] = [
        AreaKind::Lawn,
        AreaKind::Path,
        AreaKind::Terrace,
        AreaKind::Pool,
        AreaKind::Pergola,
        AreaKind::Loggia,
        AreaKind::Hedge,
        AreaKind::Courtyard,
        AreaKind::Parterre,
        AreaKind::Street,
        AreaKind::Piazza,
        AreaKind::Road,
        AreaKind::Vineyard,
        AreaKind::Tower,
        AreaKind::Platform,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AreaKind::Lawn => "Lawn",
            AreaKind::Path => "Path",
            AreaKind::Terrace => "Terrace",
            AreaKind::Pool => "Pool",
            AreaKind::Pergola => "Pergola",
            AreaKind::Loggia => "Loggia",
            AreaKind::Hedge => "Hedge",
            AreaKind::Courtyard => "Courtyard",
            AreaKind::Parterre => "Parterre",
            AreaKind::Street => "Cobbled street",
            AreaKind::Piazza => "Piazza",
            AreaKind::Road => "Dirt road",
            AreaKind::Vineyard => "Vineyard",
            AreaKind::Tower => "Belvedere tower",
            AreaKind::Platform => "Platform",
        }
    }

    /// Height used when the part is placed by hand.
    pub fn default_height(self) -> f64 {
        match self {
            AreaKind::Lawn => 0.1,
            AreaKind::Path | AreaKind::Road => 0.04,
            AreaKind::Terrace | AreaKind::Street | AreaKind::Piazza => 0.05,
            AreaKind::Pool => 1.4,
            AreaKind::Pergola => 2.6,
            AreaKind::Loggia | AreaKind::Courtyard => 3.6,
            AreaKind::Hedge => 1.1,
            AreaKind::Parterre => 0.6,
            AreaKind::Vineyard => 1.5,
            AreaKind::Tower => 12.0,
            AreaKind::Platform => 1.0,
        }
    }

    /// Thin ground coverings that are draped over the terrain.
    pub fn drapes(self) -> bool {
        matches!(
            self,
            AreaKind::Lawn
                | AreaKind::Terrace
                | AreaKind::Path
                | AreaKind::Street
                | AreaKind::Road
                | AreaKind::Vineyard
        )
    }
}

/// A rectangular garden or landscape element.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AreaPart {
    pub kind: AreaKind,
    pub rect: Rect,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropKind {
    OliveTree,
    CypressTree,
    LemonTree,
    Pot,
    Fountain,
    Bench,
    Column,
    Lamp,
    /// A stone wellhead with an iron arch.
    Well,
    /// A stone urn on a pedestal.
    Urn,
    /// An Italian stone pine with a flat umbrella crown.
    StonePine,
    /// A date palm, common in Palermo gardens.
    Palm,
}

impl PropKind {
    pub const ALL: [PropKind; 12] = [
        PropKind::OliveTree,
        PropKind::CypressTree,
        PropKind::LemonTree,
        PropKind::StonePine,
        PropKind::Palm,
        PropKind::Pot,
        PropKind::Fountain,
        PropKind::Well,
        PropKind::Urn,
        PropKind::Bench,
        PropKind::Column,
        PropKind::Lamp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PropKind::OliveTree => "Olive tree",
            PropKind::CypressTree => "Cypress",
            PropKind::LemonTree => "Lemon tree",
            PropKind::Pot => "Terracotta pot",
            PropKind::Fountain => "Fountain",
            PropKind::Bench => "Stone bench",
            PropKind::Column => "Column",
            PropKind::Lamp => "Lantern",
            PropKind::Well => "Well",
            PropKind::Urn => "Urn on pedestal",
            PropKind::StonePine => "Stone pine",
            PropKind::Palm => "Palm",
        }
    }

    /// Footprint width at scale 1, in metres.
    pub fn base_size(self) -> f64 {
        match self {
            PropKind::OliveTree => 2.6,
            PropKind::CypressTree => 1.0,
            PropKind::LemonTree => 1.8,
            PropKind::Pot => 0.6,
            PropKind::Fountain => 2.0,
            PropKind::Bench => 1.6,
            PropKind::Column => 0.5,
            PropKind::Lamp => 0.4,
            PropKind::Well => 1.4,
            PropKind::Urn => 0.7,
            PropKind::StonePine => 5.0,
            PropKind::Palm => 2.4,
        }
    }

    pub fn default_height(self) -> f64 {
        match self {
            PropKind::OliveTree => 4.0,
            PropKind::CypressTree => 7.0,
            PropKind::LemonTree => 3.0,
            PropKind::Pot => 0.6,
            PropKind::Fountain => 1.5,
            PropKind::Bench => 0.5,
            PropKind::Column => 3.2,
            PropKind::Lamp => 2.6,
            PropKind::Well => 2.2,
            PropKind::Urn => 1.6,
            PropKind::StonePine => 9.0,
            PropKind::Palm => 6.5,
        }
    }

    pub fn is_tree(self) -> bool {
        matches!(
            self,
            PropKind::OliveTree
                | PropKind::CypressTree
                | PropKind::LemonTree
                | PropKind::StonePine
                | PropKind::Palm
        )
    }
}

/// A free-standing object: trees, pots, fountains, furniture.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropPart {
    pub kind: PropKind,
    pub at: P2,
    /// Rotation about the vertical axis, in radians.
    pub yaw: f64,
    pub scale: f64,
    pub height: f64,
}

impl Part {
    /// A ground-plane box around the part, for selection and bounds.
    pub fn footprint(&self) -> Rect {
        match &self.kind {
            PartKind::Wall(w) => {
                let h = w.thickness / 2.0;
                let (x0, x1) = (w.a[0].min(w.b[0]) - h, w.a[0].max(w.b[0]) + h);
                let (z0, z1) = (w.a[1].min(w.b[1]) - h, w.a[1].max(w.b[1]) + h);
                Rect::new(x0, z0, x1 - x0, z1 - z0)
            }
            PartKind::Opening(o) => square(o.at, o.width.max(0.4)),
            PartKind::Balcony(b) => {
                let c = [
                    b.at[0] + b.facing[0] * b.depth / 2.0,
                    b.at[1] + b.facing[1] * b.depth / 2.0,
                ];
                square(c, b.width.max(b.depth))
            }
            PartKind::Floor(f) => f.rect,
            PartKind::Stairs(s) => s.rect,
            PartKind::Roof(r) => match r.roof {
                Roof::Hip { overhang, .. } => r.rect.grow(overhang),
                Roof::Flat { .. } => r.rect,
            },
            PartKind::Area(a) => a.rect,
            PartKind::Prop(p) => square(p.at, p.kind.base_size() * p.scale),
        }
    }

    pub fn label(&self) -> String {
        match &self.kind {
            PartKind::Wall(w) if w.finish != WallFinish::Building => w.finish.label().into(),
            PartKind::Wall(w) if w.exterior => "Facade wall".into(),
            PartKind::Wall(_) => "Partition wall".into(),
            PartKind::Opening(o) => o.kind.label().into(),
            PartKind::Balcony(_) => "Balcony".into(),
            PartKind::Floor(f) => f
                .room
                .map_or("Floor".into(), |r| format!("{} floor", r.label())),
            PartKind::Stairs(_) => "Stairs".into(),
            PartKind::Roof(r) => format!("Roof ({})", r.roof.label()),
            PartKind::Area(a) => a.kind.label().into(),
            PartKind::Prop(p) => p.kind.label().into(),
        }
    }

    /// Moves the part across the ground by `d`.
    pub fn translate(&mut self, d: P2) {
        let shift = |p: &mut P2| {
            p[0] += d[0];
            p[1] += d[1];
        };
        let shift_rect = |r: &mut Rect| {
            r.x += d[0];
            r.z += d[1];
        };
        match &mut self.kind {
            PartKind::Wall(w) => {
                shift(&mut w.a);
                shift(&mut w.b);
            }
            PartKind::Opening(o) => shift(&mut o.at),
            PartKind::Balcony(b) => shift(&mut b.at),
            PartKind::Floor(f) => shift_rect(&mut f.rect),
            PartKind::Stairs(s) => shift_rect(&mut s.rect),
            PartKind::Roof(r) => shift_rect(&mut r.rect),
            PartKind::Area(a) => shift_rect(&mut a.rect),
            PartKind::Prop(p) => shift(&mut p.at),
        }
    }

    /// Turns the part half way round `centre`, so a building can face the other way.
    pub fn rotate_half_about(&mut self, centre: P2) {
        let flip = |p: &mut P2| *p = [2.0 * centre[0] - p[0], 2.0 * centre[1] - p[1]];
        let flip_dir = |v: &mut P2| *v = [-v[0], -v[1]];
        let flip_rect = |r: &mut Rect| {
            *r = Rect::new(2.0 * centre[0] - r.x1(), 2.0 * centre[1] - r.z1(), r.w, r.d)
        };
        match &mut self.kind {
            PartKind::Wall(w) => {
                flip(&mut w.a);
                flip(&mut w.b);
            }
            PartKind::Opening(o) => {
                flip(&mut o.at);
                flip_dir(&mut o.facing);
            }
            PartKind::Balcony(b) => {
                flip(&mut b.at);
                flip_dir(&mut b.facing);
            }
            PartKind::Floor(f) => flip_rect(&mut f.rect),
            PartKind::Stairs(s) => {
                flip_rect(&mut s.rect);
                s.heading = s.heading.turned_right().turned_right();
            }
            PartKind::Roof(r) => {
                flip_rect(&mut r.rect);
                r.open = [r.open[1], r.open[0], r.open[3], r.open[2]];
            }
            PartKind::Area(a) => flip_rect(&mut a.rect),
            PartKind::Prop(p) => {
                flip(&mut p.at);
                p.yaw += std::f64::consts::PI;
            }
        }
    }

    /// Rotates the part about its own centre by a quarter turn clockwise (seen from above).
    pub fn rotate_quarter(&mut self) {
        let c = self.footprint().center();
        self.rotate_quarter_about([c.0, c.1]);
    }

    /// Rotates the part about `centre` by a quarter turn clockwise (seen from above), so a
    /// whole building can turn as one: a house facing south ends up facing west.
    pub fn rotate_quarter_about(&mut self, centre: P2) {
        let turn = |p: &mut P2| {
            let (dx, dz) = (p[0] - centre[0], p[1] - centre[1]);
            *p = [centre[0] + dz, centre[1] - dx];
        };
        let turn_dir = |v: &mut P2| *v = [v[1], -v[0]];
        let turn_rect = |r: &mut Rect| {
            *r = Rect::new(
                centre[0] + r.z - centre[1],
                centre[1] - r.x1() + centre[0],
                r.d,
                r.w,
            )
        };
        match &mut self.kind {
            PartKind::Wall(w) => {
                turn(&mut w.a);
                turn(&mut w.b);
            }
            PartKind::Opening(o) => {
                turn(&mut o.at);
                turn_dir(&mut o.facing);
            }
            PartKind::Balcony(b) => {
                turn(&mut b.at);
                turn_dir(&mut b.facing);
            }
            PartKind::Floor(f) => turn_rect(&mut f.rect),
            PartKind::Stairs(s) => {
                turn_rect(&mut s.rect);
                s.heading = s.heading.turned_right();
            }
            PartKind::Roof(r) => {
                turn_rect(&mut r.rect);
                // (south, north, west, east): south turns to west, west to north, and so on.
                r.open = [r.open[3], r.open[2], r.open[0], r.open[1]];
            }
            PartKind::Area(a) => turn_rect(&mut a.rect),
            PartKind::Prop(p) => {
                turn(&mut p.at);
                p.yaw -= std::f64::consts::FRAC_PI_2;
            }
        }
    }
}

fn square(c: P2, size: f64) -> Rect {
    Rect::new(c[0] - size / 2.0, c[1] - size / 2.0, size, size)
}

pub fn dist(a: P2, b: P2) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// A building as a list of parts, plus the style and colours that dress them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub format: u32,
    pub name: String,
    pub description: String,
    pub style: StyleProfile,
    /// The default palette, for parts outside any building.
    pub palette: Palette,
    pub parts: Vec<Part>,
    pub next_id: PartId,
    /// Each building's own colours, indexed by `Part::group`.
    #[serde(default)]
    pub palettes: Vec<Palette>,
    /// The ground, if the site has relief.
    #[serde(default)]
    pub terrain: Option<crate::terrain::Terrain>,
    /// A sea, lake or river lying on the terrain.
    #[serde(default)]
    pub water: Option<crate::landscape::Water>,
}

impl Layout {
    pub fn new(name: impl Into<String>, style: StyleProfile, palette: Palette) -> Self {
        Self {
            format: FORMAT_VERSION,
            name: name.into(),
            description: String::new(),
            style,
            palette,
            parts: Vec::new(),
            next_id: 1,
            palettes: Vec::new(),
            terrain: None,
            water: None,
        }
    }

    /// Adds a part standing at `ground` height as part of building `group`.
    pub fn add_in(
        &mut self,
        level: u32,
        ground: f64,
        group: Option<u32>,
        kind: PartKind,
    ) -> PartId {
        let id = self.next_id;
        self.next_id += 1;
        self.parts.push(Part {
            id,
            level,
            ground,
            group,
            kind,
        });
        id
    }

    /// The colours of building `group` (or the default palette).
    pub fn palette_of(&self, group: Option<u32>) -> &Palette {
        group
            .and_then(|g| self.palettes.get(g as usize))
            .unwrap_or(&self.palette)
    }

    /// Registers a new building's palette and returns its group.
    pub fn new_group(&mut self, palette: Palette) -> u32 {
        self.palettes.push(palette);
        (self.palettes.len() - 1) as u32
    }

    /// Every part of building `group`.
    pub fn group_parts(&self, group: u32) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(move |p| p.group == Some(group))
    }

    /// Height of a part's base (the underside of its level's slab) above sea level.
    pub fn base(&self, part: &Part) -> f64 {
        part.ground + self.elevation(part.level)
    }

    /// Height of the terrain at `p` (zero without terrain).
    pub fn ground_at(&self, p: P2) -> f64 {
        self.terrain.as_ref().map_or(0.0, |t| t.height_at(p))
    }

    pub fn add(&mut self, level: u32, kind: PartKind) -> PartId {
        let id = self.next_id;
        self.next_id += 1;
        self.parts.push(Part {
            id,
            level,
            ground: 0.0,
            group: None,
            kind,
        });
        id
    }

    /// Puts back a part that was removed, keeping its id.
    pub fn restore(&mut self, part: Part) {
        self.next_id = self.next_id.max(part.id + 1);
        self.parts.push(part);
    }

    pub fn remove(&mut self, id: PartId) -> Option<Part> {
        let index = self.parts.iter().position(|p| p.id == id)?;
        Some(self.parts.remove(index))
    }

    pub fn get(&self, id: PartId) -> Option<&Part> {
        self.parts.iter().find(|p| p.id == id)
    }

    pub fn get_mut(&mut self, id: PartId) -> Option<&mut Part> {
        self.parts.iter_mut().find(|p| p.id == id)
    }

    /// Number of storeys: one more than the highest level holding a wall or floor.
    pub fn levels(&self) -> u32 {
        self.parts
            .iter()
            .filter(|p| matches!(p.kind, PartKind::Wall(_) | PartKind::Floor(_)))
            .map(|p| p.level + 1)
            .max()
            .unwrap_or(1)
    }

    /// Height of a level's base (the underside of its floor slab) above the ground.
    pub fn elevation(&self, level: u32) -> f64 {
        level as f64 * self.style.floor_height
    }

    pub fn on_level(&self, level: u32) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(move |p| p.level == level)
    }

    /// The box around every part on the ground plane.
    pub fn bounds(&self) -> Rect {
        let mut parts = self.parts.iter().map(Part::footprint);
        let Some(first) = parts.next() else {
            return Rect::new(-5.0, -5.0, 10.0, 10.0);
        };
        parts.fold(first, |a, b| {
            let (x0, z0) = (a.x.min(b.x), a.z.min(b.z));
            let (x1, z1) = (a.x1().max(b.x1()), a.z1().max(b.z1()));
            Rect::new(x0, z0, x1 - x0, z1 - z0)
        })
    }

    /// The wall on `level` that `at` lies on, if any: the closest one whose body contains it.
    pub fn host_wall(&self, level: u32, at: P2) -> Option<(&Part, &Wall)> {
        self.host_wall_in(level, None, at)
    }

    /// Like [`Layout::host_wall`], but only walls of building `group` when it is given.
    pub fn host_wall_in(&self, level: u32, group: Option<u32>, at: P2) -> Option<(&Part, &Wall)> {
        self.on_level(level)
            .filter(move |p| group.is_none() || p.group == group)
            .filter_map(|p| match &p.kind {
                PartKind::Wall(w) => Some((p, w)),
                _ => None,
            })
            .map(|(p, w)| (p, w, w.closest(at).1))
            .filter(|(_, w, d)| *d <= w.thickness / 2.0 + 0.05)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(p, w, _)| (p, w))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a layout always serialises to JSON")
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}
