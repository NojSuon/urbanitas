//! Build mode: the parts catalogue, Sims-style placement with grid snapping (or free
//! placement while Alt is held), whole-building selection, terrain sculpting, moving,
//! rotating, deleting and the ghost preview.

use std::f64::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use urbanitas::geometry::Rect;
use urbanitas::layout::append_building;
use urbanitas::mesh::build::{Scene, View, WallMode};
use urbanitas::model::{OpeningKind, Roof};
use urbanitas::parts::*;
use urbanitas::style::OpeningSpec;
use urbanitas::terrain::{Brush, Terrain};
use urbanitas::{BuildingParams, generate};

use crate::project::{Mode, Project};
use crate::scene::{MainCamera, bevy_mesh, merged};

/// Snapping grid, in metres.
pub const GRID: f64 = 0.5;
/// How close the pointer must be to a wall to mount an opening on it.
const MOUNT_REACH: f64 = 1.2;

pub struct ToolsPlugin;

impl Plugin for ToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tool>()
            .init_resource::<Pointer>()
            .add_systems(Startup, setup_ghost_materials)
            .add_systems(
                Update,
                (
                    track_pointer,
                    keyboard,
                    mouse,
                    sculpt,
                    update_ghost,
                    draw_gizmos,
                )
                    .chain(),
            );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Walls,
    Openings,
    Structure,
    Roofs,
    Garden,
    Streets,
    Terrain,
    Buildings,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Buildings,
        Category::Walls,
        Category::Openings,
        Category::Structure,
        Category::Roofs,
        Category::Garden,
        Category::Streets,
        Category::Terrain,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Walls => "Walls",
            Category::Openings => "Openings",
            Category::Structure => "Structure",
            Category::Roofs => "Roofs",
            Category::Garden => "Garden",
            Category::Streets => "Streets",
            Category::Terrain => "Terrain",
            Category::Buildings => "Buildings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sculpt {
    Raise,
    Lower,
    Flatten,
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Wall(WallFinish, bool),
    ArchedWindow,
    SquareWindow,
    SmallWindow,
    FrontDoor,
    BalconyDoor,
    Storefront,
    Archway,
    Gate,
    Balcony,
    Floor,
    Stairs,
    HipRoof,
    FlatRoof,
    Area(AreaKind),
    Prop(PropKind),
    Terrain(Sculpt),
    /// A whole generated building, from the current parameters.
    Building,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Click and drag from one end to the other.
    Line,
    /// Click and drag from corner to corner.
    Rect,
    /// Click on a wall.
    Mounted,
    /// Click anywhere.
    Point,
    /// Paint with the mouse held down.
    Brush,
}

impl Item {
    pub fn catalogue(category: Category) -> Vec<Item> {
        use AreaKind as A;
        use PropKind as P;
        match category {
            Category::Buildings => vec![Item::Building],
            Category::Walls => vec![
                Item::Wall(WallFinish::Building, true),
                Item::Wall(WallFinish::Building, false),
                Item::Wall(WallFinish::Garden, false),
                Item::Wall(WallFinish::Fence, false),
                Item::Wall(WallFinish::DryStone, false),
            ],
            Category::Openings => vec![
                Item::ArchedWindow,
                Item::SquareWindow,
                Item::SmallWindow,
                Item::FrontDoor,
                Item::BalconyDoor,
                Item::Storefront,
                Item::Archway,
                Item::Balcony,
                Item::Gate,
            ],
            Category::Structure => vec![
                Item::Floor,
                Item::Stairs,
                Item::Prop(P::Column),
                Item::Area(A::Loggia),
                Item::Area(A::Courtyard),
                Item::Area(A::Tower),
                Item::Area(A::Platform),
            ],
            Category::Roofs => vec![Item::HipRoof, Item::FlatRoof],
            Category::Garden => vec![
                Item::Area(A::Lawn),
                Item::Area(A::Path),
                Item::Area(A::Terrace),
                Item::Area(A::Pool),
                Item::Area(A::Pergola),
                Item::Area(A::Hedge),
                Item::Area(A::Parterre),
                Item::Area(A::Vineyard),
                Item::Prop(P::OliveTree),
                Item::Prop(P::CypressTree),
                Item::Prop(P::LemonTree),
                Item::Prop(P::StonePine),
                Item::Prop(P::Palm),
                Item::Prop(P::Pot),
            ],
            Category::Streets => vec![
                Item::Area(A::Street),
                Item::Area(A::Piazza),
                Item::Area(A::Road),
                Item::Prop(P::Fountain),
                Item::Prop(P::Well),
                Item::Prop(P::Urn),
                Item::Prop(P::Bench),
                Item::Prop(P::Lamp),
            ],
            Category::Terrain => vec![
                Item::Terrain(Sculpt::Raise),
                Item::Terrain(Sculpt::Lower),
                Item::Terrain(Sculpt::Flatten),
                Item::Terrain(Sculpt::Smooth),
            ],
        }
    }

    pub fn all() -> Vec<Item> {
        Category::ALL
            .iter()
            .flat_map(|c| Item::catalogue(*c))
            .collect()
    }

    pub fn label(self) -> &'static str {
        match self {
            Item::Wall(WallFinish::Building, true) => "Facade wall",
            Item::Wall(WallFinish::Building, false) => "Partition",
            Item::Wall(finish, _) => finish.label(),
            Item::ArchedWindow => "Arched window",
            Item::SquareWindow => "Square window",
            Item::SmallWindow => "Small window",
            Item::FrontDoor => "Front door",
            Item::BalconyDoor => "Balcony door",
            Item::Storefront => "Storefront",
            Item::Archway => "Archway",
            Item::Gate => "Gate",
            Item::Balcony => "Balcony",
            Item::Floor => "Floor",
            Item::Stairs => "Stairs",
            Item::HipRoof => "Hipped roof",
            Item::FlatRoof => "Roof terrace",
            Item::Area(kind) => kind.label(),
            Item::Prop(kind) => kind.label(),
            Item::Terrain(Sculpt::Raise) => "Raise",
            Item::Terrain(Sculpt::Lower) => "Lower",
            Item::Terrain(Sculpt::Flatten) => "Flatten",
            Item::Terrain(Sculpt::Smooth) => "Smooth",
            Item::Building => "New building",
        }
    }

    pub fn placement(self) -> Placement {
        match self {
            Item::Wall(..) => Placement::Line,
            Item::Floor | Item::HipRoof | Item::FlatRoof | Item::Area(_) => Placement::Rect,
            Item::ArchedWindow
            | Item::SquareWindow
            | Item::SmallWindow
            | Item::FrontDoor
            | Item::BalconyDoor
            | Item::Storefront
            | Item::Archway
            | Item::Gate
            | Item::Balcony => Placement::Mounted,
            Item::Stairs | Item::Prop(_) | Item::Building => Placement::Point,
            Item::Terrain(_) => Placement::Brush,
        }
    }

    pub fn hint(self) -> &'static str {
        match self.placement() {
            Placement::Line => "Drag along the grid to draw a wall",
            Placement::Rect => "Drag from corner to corner",
            Placement::Mounted => "Point at a wall and click",
            Placement::Point if self == Item::Building => {
                "Click to place a building from the Generate settings"
            }
            Placement::Point => "Click to place · R to turn",
            Placement::Brush => "Hold the left button and paint the ground",
        }
    }
}

#[derive(Resource)]
pub struct Tool {
    /// `None` is the select tool.
    pub item: Option<Item>,
    pub category: Category,
    pub yaw: f64,
    pub heading: Heading,
    pub brush_radius: f64,
    pub brush_strength: f64,
    drag_start: Option<P2>,
    /// The part (or whole building) being dragged and where it was grabbed.
    moving: Option<(Grab, P2)>,
    /// The height a flatten stroke levels to.
    flatten_to: Option<f64>,
    stroking: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Part(PartId),
    Group(u32),
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            item: None,
            category: Category::Buildings,
            yaw: 0.0,
            heading: Heading::North,
            brush_radius: 6.0,
            brush_strength: 0.6,
            drag_start: None,
            moving: None,
            flatten_to: None,
            stroking: false,
        }
    }
}

impl Tool {
    pub fn pick(&mut self, item: Option<Item>) {
        self.item = item;
        self.drag_start = None;
        self.moving = None;
        self.stroking = false;
    }
}

/// Where the pointer meets the ground (or the active level's floor).
#[derive(Resource, Default)]
pub struct Pointer {
    pub ground: Option<P2>,
    /// The height under the pointer.
    pub height: f64,
    /// Alt held: no snapping, fine rotation.
    pub free: bool,
    /// Shift held: select whole buildings.
    pub whole: bool,
    pub over_ui: bool,
    pub ui_wants_keyboard: bool,
    /// Pins the pointer to a spot (scripted screenshots).
    pub forced: Option<P2>,
}

#[derive(Component)]
struct Ghost;

#[derive(Resource)]
struct GhostMaterials {
    ok: Handle<StandardMaterial>,
    bad: Handle<StandardMaterial>,
}

fn setup_ghost_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let ghost = |r, g, b| StandardMaterial {
        base_color: Color::srgba(r, g, b, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    };
    commands.insert_resource(GhostMaterials {
        ok: materials.add(ghost(0.55, 0.85, 0.45)),
        bad: materials.add(ghost(0.9, 0.35, 0.25)),
    });
}

fn snap(p: P2, free: bool) -> P2 {
    if free {
        p
    } else {
        [(p[0] / GRID).round() * GRID, (p[1] / GRID).round() * GRID]
    }
}

fn walls_on(layout: &Layout, level: u32) -> impl Iterator<Item = (&Part, &Wall)> {
    layout.on_level(level).filter_map(|p| match &p.kind {
        PartKind::Wall(w) => Some((p, w)),
        _ => None,
    })
}

/// The building whose ground floor covers `p`, with its ground height; else the terrain there.
pub fn ground_for(layout: &Layout, p: P2) -> (f64, Option<u32>) {
    let building = layout.parts.iter().find(|part| {
        part.group.is_some()
            && part.level == 0
            && matches!(
                &part.kind,
                PartKind::Floor(_)
                    | PartKind::Area(AreaPart {
                        kind: AreaKind::Lawn | AreaKind::Terrace,
                        ..
                    })
            )
            && {
                let r = part.footprint();
                p[0] >= r.x && p[0] <= r.x1() && p[1] >= r.z && p[1] <= r.z1()
            }
    });
    match building {
        Some(part) => (part.ground, part.group),
        None => (layout.ground_at(p), None),
    }
}

/// Snaps to a nearby wall end so new walls join old ones cleanly; otherwise to the grid.
fn snap_wall_end(layout: &Layout, level: u32, p: P2, free: bool) -> P2 {
    walls_on(layout, level)
        .flat_map(|(_, w)| [w.a, w.b])
        .map(|q| (q, dist(p, q)))
        .filter(|(_, d)| *d < 0.35)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or_else(|| snap(p, free), |(q, _)| q)
}

fn level_centre(layout: &Layout, level: u32, group: Option<u32>) -> P2 {
    let floors: Vec<Rect> = layout
        .on_level(level)
        .filter(|p| p.group == group)
        .filter_map(|p| match &p.kind {
            PartKind::Floor(f) => Some(f.rect),
            _ => None,
        })
        .collect();
    let area: f64 = floors.iter().map(Rect::area).sum();
    if area > 0.0 {
        let x = floors.iter().map(|r| r.center().0 * r.area()).sum::<f64>() / area;
        let z = floors.iter().map(|r| r.center().1 * r.area()).sum::<f64>() / area;
        [x, z]
    } else {
        let (x, z) = layout.bounds().center();
        [x, z]
    }
}

/// What the current tool would place at the pointer.
#[derive(Clone, Debug, PartialEq)]
struct Candidate {
    level: u32,
    ground: f64,
    group: Option<u32>,
    kind: PartKind,
    valid: bool,
}

fn candidate(project: &Project, tool: &Tool, pointer: &Pointer) -> Option<Candidate> {
    let layout = &project.layout;
    let style = &layout.style;
    let level = project.level;
    let raw = pointer.ground?;
    let free = pointer.free;

    if let Some((Grab::Part(id), grab)) = tool.moving {
        let part = layout.get(id)?;
        let mut moved = part.clone();
        let (from, to) = (snap(grab, free), snap(raw, free));
        moved.translate([to[0] - from[0], to[1] - from[1]]);
        remount(layout, &mut moved);
        return Some(Candidate {
            level: moved.level,
            ground: moved.ground,
            group: moved.group,
            kind: moved.kind,
            valid: true,
        });
    }

    let item = tool.item?;
    let (ground, group) = ground_for(layout, raw);
    match item.placement() {
        Placement::Brush => None,
        Placement::Line => {
            let start = tool.drag_start?;
            let end = snap_wall_end(layout, level, raw, free);
            let Item::Wall(finish, exterior) = item else {
                return None;
            };
            let (height, thickness) = match finish {
                WallFinish::Building if exterior => (style.floor_height, style.wall_thickness),
                WallFinish::Building => (style.floor_height, style.partition_thickness),
                f => (f.default_height(), f.default_thickness()),
            };
            let wall = Wall {
                a: start,
                b: end,
                height,
                thickness,
                exterior,
                base: 0.0,
                finish,
            };
            let (ground, group) = ground_for(layout, start);
            Some(Candidate {
                level,
                ground,
                group,
                kind: PartKind::Wall(wall),
                valid: wall.length() >= 0.5,
            })
        }
        Placement::Rect => {
            let end = snap(raw, free);
            let start = tool.drag_start.unwrap_or(end);
            let (x0, x1) = (
                start[0].min(end[0]),
                start[0].max(end[0]).max(start[0].min(end[0]) + GRID),
            );
            let (z0, z1) = (
                start[1].min(end[1]),
                start[1].max(end[1]).max(start[1].min(end[1]) + GRID),
            );
            let rect = Rect::new(x0, z0, x1 - x0, z1 - z0);
            let kind = match item {
                Item::Floor => PartKind::Floor(FloorPart { rect, room: None }),
                Item::HipRoof => PartKind::Roof(RoofPart {
                    rect,
                    roof: Roof::hip(&rect, style.roof_pitch_deg, style.eave_overhang),
                    open: [false; 4],
                }),
                Item::FlatRoof => PartKind::Roof(RoofPart {
                    rect,
                    roof: Roof::Flat {
                        parapet_height: style.parapet_height,
                        balustrade: style.roof_balustrade,
                    },
                    open: [false; 4],
                }),
                Item::Area(kind) => {
                    let height = if matches!(kind, AreaKind::Loggia | AreaKind::Courtyard) {
                        style.floor_height
                    } else {
                        kind.default_height()
                    };
                    PartKind::Area(AreaPart { kind, rect, height })
                }
                _ => return None,
            };
            let (ground, group) = ground_for(layout, [rect.center().0, rect.center().1]);
            Some(Candidate {
                level,
                ground,
                group,
                kind,
                valid: rect.w >= 0.5 && rect.d >= 0.5,
            })
        }
        Placement::Mounted => {
            mounted(project, item, raw, free).map(|(kind, valid, host_ground, host_group)| {
                Candidate {
                    level,
                    ground: host_ground,
                    group: host_group,
                    kind,
                    valid,
                }
            })
        }
        Placement::Point => {
            let at = snap(raw, free);
            let kind = match item {
                Item::Stairs => {
                    let (w, l) = (2.4, 4.8);
                    let (rw, rd) = match tool.heading {
                        Heading::North | Heading::South => (w, l),
                        Heading::East | Heading::West => (l, w),
                    };
                    PartKind::Stairs(StairsPart {
                        rect: Rect::new(at[0] - rw / 2.0, at[1] - rd / 2.0, rw, rd),
                        rise: style.floor_height,
                        heading: tool.heading,
                    })
                }
                Item::Prop(kind) => {
                    let height = if kind == PropKind::Column {
                        style.floor_height - 0.2
                    } else {
                        kind.default_height()
                    };
                    PartKind::Prop(PropPart {
                        kind,
                        at,
                        yaw: tool.yaw,
                        scale: 1.0,
                        height,
                    })
                }
                _ => return None,
            };
            Some(Candidate {
                level,
                ground,
                group,
                kind,
                valid: true,
            })
        }
    }
}

/// An opening, gate or balcony on the wall nearest the pointer, kept inside the wall and clear of
/// other openings. Returns the part, whether it fits, and the host wall's ground and building.
fn mounted(
    project: &Project,
    item: Item,
    p: P2,
    free: bool,
) -> Option<(PartKind, bool, f64, Option<u32>)> {
    let layout = &project.layout;
    let style = &layout.style;
    let level = project.level;
    let (host, wall, along) = walls_on(layout, level)
        .map(|(part, w)| {
            let (along, d) = w.closest(p);
            (part, w, along, d)
        })
        .filter(|(.., d)| *d < MOUNT_REACH)
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(part, w, along, _)| (part, w, along))?;

    let spec = match item {
        Item::ArchedWindow => style.window,
        Item::SquareWindow => OpeningSpec {
            arched: false,
            ..style.window
        },
        Item::SmallWindow => style.small_window,
        Item::FrontDoor => style.entrance,
        Item::BalconyDoor => style.balcony_door,
        Item::Storefront => style.storefront,
        Item::Archway => style.interior_door,
        Item::Gate => OpeningSpec {
            width: 2.4,
            height: wall.height,
            sill: 0.0,
            arched: false,
        },
        _ => OpeningSpec {
            width: 2.4,
            height: 0.0,
            sill: 0.0,
            arched: false,
        },
    };
    let len = wall.length();
    let half = spec.width / 2.0;
    let mut valid = len >= spec.width + 0.3;
    let along = if free {
        along
    } else {
        (along / 0.25).round() * 0.25
    };
    let along = along.clamp(half + 0.15, (len - half - 0.15).max(half + 0.15));
    let at = wall.point_at(along);

    let d = wall.dir();
    let normal = [-d[1], d[0]];
    let toward = if wall.exterior {
        let c = level_centre(layout, level, host.group);
        [at[0] - c[0], at[1] - c[1]]
    } else {
        [p[0] - at[0], p[1] - at[1]]
    };
    let sign = if normal[0] * toward[0] + normal[1] * toward[1] >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let facing = [normal[0] * sign, normal[1] * sign];

    for other in layout.on_level(level) {
        let (o_at, o_width) = match &other.kind {
            PartKind::Opening(o) => (o.at, o.width),
            PartKind::Balcony(b) if item == Item::Balcony => (b.at, b.width),
            _ => continue,
        };
        if layout
            .host_wall_in(level, other.group, o_at)
            .map(|(h, _)| h.id)
            != Some(host.id)
        {
            continue;
        }
        let (o_along, _) = wall.closest(o_at);
        if (o_along - along).abs() < (o_width + spec.width) / 2.0 + 0.2 {
            valid = false;
        }
    }

    if item == Item::Balcony {
        return Some((
            PartKind::Balcony(BalconyPart {
                at,
                width: spec.width,
                depth: style.balcony_depth,
                facing,
            }),
            valid,
            host.ground,
            host.group,
        ));
    }
    let gate = item == Item::Gate;
    let sill = if gate {
        0.0
    } else {
        spec.sill + style.slab_thickness
    };
    valid &= gate || sill + spec.height <= wall.height + wall.base - 0.05;
    let kind = match item {
        Item::FrontDoor => OpeningKind::Door,
        Item::BalconyDoor => OpeningKind::BalconyDoor,
        Item::Storefront => OpeningKind::Storefront,
        Item::Archway => OpeningKind::Archway,
        Item::Gate => OpeningKind::Gate,
        _ => OpeningKind::Window,
    };
    let opening = OpeningPart {
        kind,
        at,
        width: spec.width,
        height: spec.height,
        sill,
        arched: spec.arched,
        facing,
    };
    Some((PartKind::Opening(opening), valid, host.ground, host.group))
}

/// Keeps a moved opening or balcony on the nearest wall.
fn remount(layout: &Layout, part: &mut Part) {
    let level = part.level;
    let at = match &mut part.kind {
        PartKind::Opening(o) => &mut o.at,
        PartKind::Balcony(b) => &mut b.at,
        _ => return,
    };
    let nearest = walls_on(layout, level)
        .map(|(_, w)| (w, w.closest(*at)))
        .filter(|(_, (_, d))| *d < MOUNT_REACH)
        .min_by(|a, b| a.1.1.total_cmp(&b.1.1));
    if let Some((w, (along, _))) = nearest {
        *at = w.point_at(along);
    }
}

/// The part under the pointer on the active level, preferring small and specific parts.
pub fn pick(layout: &Layout, level: u32, p: P2) -> Option<PartId> {
    let rank = |part: &Part| match &part.kind {
        PartKind::Opening(_) => 0,
        PartKind::Balcony(_) => 1,
        PartKind::Prop(_) => 2,
        PartKind::Stairs(_) => 3,
        PartKind::Wall(_) => 4,
        PartKind::Area(a) if !matches!(a.kind, AreaKind::Lawn | AreaKind::Platform) => 5,
        PartKind::Floor(_) => 6,
        PartKind::Roof(_) => 7,
        PartKind::Area(_) => 8,
    };
    layout
        .on_level(level)
        .filter(|part| {
            let r = part.footprint().grow(0.1);
            p[0] >= r.x && p[0] <= r.x1() && p[1] >= r.z && p[1] <= r.z1()
        })
        .filter(|part| match &part.kind {
            PartKind::Wall(w) => w.closest(p).1 <= w.thickness / 2.0 + 0.15,
            _ => true,
        })
        .min_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then(a.footprint().area().total_cmp(&b.footprint().area()))
        })
        .map(|part| part.id)
}

/// The box round every part of a building, on the ground.
pub fn group_bounds(layout: &Layout, group: u32) -> Option<Rect> {
    layout
        .group_parts(group)
        .filter(|p| p.level == 0)
        .map(Part::footprint)
        .reduce(|a, b| {
            let (x0, z0) = (a.x.min(b.x), a.z.min(b.z));
            let (x1, z1) = (a.x1().max(b.x1()), a.z1().max(b.z1()));
            Rect::new(x0, z0, x1 - x0, z1 - z0)
        })
}

/// Height of the surface the pointer travels over at `p`: the terrain outside buildings, the
/// active floor inside them.
fn surface_height(project: &Project, p: P2) -> f64 {
    let (ground, group) = ground_for(&project.layout, p);
    let level = project.level;
    if level == 0 && group.is_none() {
        return ground;
    }
    ground
        + project.layout.elevation(level)
        + if level > 0 {
            project.layout.style.slab_thickness
        } else {
            0.0
        }
}

fn track_pointer(
    mut pointer: ResMut<Pointer>,
    project: Res<Project>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    pointer.free = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    pointer.whole = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if let Some(forced) = pointer.forced {
        pointer.ground = Some(forced);
        pointer.height = surface_height(&project, forced);
        return;
    }
    pointer.ground = None;
    let (Ok(window), Ok((camera, transform))) = (window.single(), camera.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(transform, cursor) else {
        return;
    };
    let (origin, dir) = (ray.origin.as_dvec3(), ray.direction.as_dvec3());
    // March along the ray until it passes under the surface, then refine.
    let above = |t: f64| {
        let q = origin + dir * t;
        q.y - surface_height(&project, [q.x, q.z])
    };
    let (mut lo, mut step) = (0.0, 0.5);
    let mut hi = None;
    while lo < 900.0 {
        if above(lo + step) < 0.0 {
            hi = Some(lo + step);
            break;
        }
        lo += step;
        step = (step * 1.08).min(4.0);
    }
    let Some(mut hi) = hi else { return };
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if above(mid) < 0.0 { hi = mid } else { lo = mid }
    }
    let hit = origin + dir * hi;
    pointer.ground = Some([hit.x, hit.z]);
    pointer.height = hit.y;
}

fn keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut project: ResMut<Project>,
    mut tool: ResMut<Tool>,
    pointer: Res<Pointer>,
) {
    if pointer.ui_wants_keyboard {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        if shift {
            project.redo()
        } else {
            project.undo()
        }
        return;
    }
    if ctrl && keys.just_pressed(KeyCode::KeyY) {
        project.redo();
        return;
    }
    if project.mode != Mode::Build {
        return;
    }
    let step = if pointer.free {
        15f64.to_radians()
    } else {
        FRAC_PI_2
    };
    if keys.just_pressed(KeyCode::KeyR) {
        match (tool.item, project.selected) {
            (None, Some(id)) => rotate_selected(&mut project, id),
            _ => {
                tool.yaw -= FRAC_PI_2;
                tool.heading = tool.heading.turned_right();
            }
        }
    }
    for (key, sign) in [(KeyCode::KeyQ, 1.0), (KeyCode::KeyE, -1.0)] {
        if keys.just_pressed(key) {
            match (tool.item, project.selected) {
                (None, Some(id)) => turn_selected_prop(&mut project, id, sign * step),
                _ => tool.yaw = (tool.yaw + sign * step).rem_euclid(2.0 * PI),
            }
        }
    }
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        if let Some(group) = project.selected_group {
            delete_group(&mut project, group);
        } else if let Some(id) = project.selected {
            delete_part(&mut project, id);
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        tool.pick(None);
        project.selected = None;
        project.selected_group = None;
    }
    if keys.just_pressed(KeyCode::PageUp) {
        let up = (project.level + 1).min(project.top_level() + 1);
        change_level(&mut project, up);
    }
    if keys.just_pressed(KeyCode::PageDown) {
        let down = project.level.saturating_sub(1);
        change_level(&mut project, down);
    }
}

pub fn change_level(project: &mut Project, level: u32) {
    if level == project.level {
        return;
    }
    let old = project.level;
    project.level = level;
    project.selected = None;
    if project.walls != WallMode::Up {
        project.mark(&[old, level]);
    }
}

pub fn delete_part(project: &mut Project, id: PartId) {
    let Some(level) = project.layout.get(id).map(|p| p.level) else {
        return;
    };
    let label = project.layout.get(id).map(Part::label).unwrap_or_default();
    project.edit(&[level], None, |layout| {
        layout.remove(id);
    });
    project.selected = None;
    project.status = format!("Removed {label}");
}

pub fn delete_group(project: &mut Project, group: u32) {
    let levels: Vec<u32> = project.layout.group_parts(group).map(|p| p.level).collect();
    project.edit(&levels, None, |layout| {
        layout.parts.retain(|p| p.group != Some(group))
    });
    project.selected = None;
    project.selected_group = None;
    project.status = format!("Removed building {}", group + 1);
}

pub fn rotate_selected(project: &mut Project, id: PartId) {
    let Some(level) = project.layout.get(id).map(|p| p.level) else {
        return;
    };
    project.edit(&[level], None, |layout| {
        if let Some(part) = layout.get_mut(id) {
            part.rotate_quarter();
        }
    });
}

fn turn_selected_prop(project: &mut Project, id: PartId, angle: f64) {
    let Some(part) = project.layout.get(id) else {
        return;
    };
    if !matches!(part.kind, PartKind::Prop(_)) {
        return;
    }
    let level = part.level;
    project.edit(&[level], Some(&format!("yaw-{id}")), |layout| {
        if let Some(Part {
            kind: PartKind::Prop(p),
            ..
        }) = layout.get_mut(id)
        {
            p.yaw += angle;
        }
    });
}

/// Places a whole building, generated from `params`, centred on `at`.
pub fn place_building(project: &mut Project, params: &BuildingParams, at: P2) {
    let building = match generate(params) {
        Ok(b) => b,
        Err(err) => {
            project.status = format!("Can't build that: {err}");
            return;
        }
    };
    let corner = [
        at[0] - building.footprint.w / 2.0,
        at[1] - building.footprint.d / 2.0,
    ];
    let lot = building
        .garden
        .as_ref()
        .map_or(building.footprint, |g| g.lot);
    let lot = Rect::new(corner[0] + lot.x, corner[1] + lot.z, lot.w, lot.d);
    let levels: Vec<u32> = (0..=building.floors.len() as u32 + 1).collect();
    project.edit(&levels, None, |layout| {
        let ground = match layout.terrain.as_mut() {
            Some(t) => {
                let (lo, hi) = t.range_in(&lot);
                let level = (lo + hi) / 2.0;
                t.flatten_pad(&lot.grow(0.3), level, 5.0);
                level
            }
            None => 0.0,
        };
        let group = layout.new_group(building.palette.clone());
        append_building(layout, &building, corner, ground, Some(group));
    });
    project.mark_terrain();
    project.status = format!(
        "Placed a {} {}",
        params.style.label(),
        params.building_type.label()
    );
}

fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    mut project: ResMut<Project>,
    mut tool: ResMut<Tool>,
    pointer: Res<Pointer>,
) {
    if project.mode != Mode::Build {
        return;
    }
    let Some(p) = pointer.ground else { return };
    let pressed = buttons.just_pressed(MouseButton::Left) && !pointer.over_ui;
    let released = buttons.just_released(MouseButton::Left);

    let Some(item) = tool.item else {
        if pressed {
            let level = project.level;
            let hit = pick(&project.layout, level, p);
            let group = hit
                .and_then(|id| project.layout.get(id))
                .and_then(|part| part.group);
            if pointer.whole && group.is_some() {
                project.selected = None;
                project.selected_group = group;
                tool.moving = group.map(|g| (Grab::Group(g), p));
                project.status = format!(
                    "Building {} · drag to move · Delete to remove",
                    group.unwrap_or(0) + 1
                );
            } else {
                project.selected = hit;
                project.selected_group = None;
                tool.moving = hit.map(|id| (Grab::Part(id), p));
                if let Some(id) = hit {
                    project.status = project.layout.get(id).map(Part::label).unwrap_or_default();
                }
            }
        }
        if released {
            if let Some((grab, from)) = tool.moving.take() {
                let (a, b) = (snap(from, pointer.free), snap(p, pointer.free));
                let delta = [b[0] - a[0], b[1] - a[1]];
                if delta[0].abs() + delta[1].abs() > 1e-6 {
                    move_grab(&mut project, grab, delta);
                }
            }
        }
        return;
    };

    match item.placement() {
        Placement::Line | Placement::Rect => {
            if pressed {
                tool.drag_start = Some(match item.placement() {
                    Placement::Line => {
                        snap_wall_end(&project.layout, project.level, p, pointer.free)
                    }
                    _ => snap(p, pointer.free),
                });
            }
            if released && tool.drag_start.is_some() {
                if let Some(c) = candidate(&project, &tool, &pointer).filter(|c| c.valid) {
                    place(&mut project, c, item);
                }
                tool.drag_start = None;
            }
        }
        Placement::Mounted | Placement::Point => {
            if pressed {
                if item == Item::Building {
                    let params = BuildingParams {
                        seed: project.layout.next_id ^ project.site.building.seed,
                        ..project.site.building.clone()
                    };
                    place_building(&mut project, &params, snap(p, pointer.free));
                    return;
                }
                match candidate(&project, &tool, &pointer) {
                    Some(c) if c.valid => place(&mut project, c, item),
                    Some(_) => project.status = "That does not fit there".into(),
                    None => project.status = "Point at a wall".into(),
                }
            }
        }
        Placement::Brush => {}
    }
}

fn move_grab(project: &mut Project, grab: Grab, delta: P2) {
    match grab {
        Grab::Part(id) => {
            let level = project.layout.get(id).map_or(0, |part| part.level);
            project.edit(&[level], None, |layout| {
                let snapshot = layout.clone();
                if let Some(part) = layout.get_mut(id) {
                    part.translate(delta);
                    remount(&snapshot, part);
                }
            });
        }
        Grab::Group(g) => {
            let levels: Vec<u32> = project.layout.group_parts(g).map(|p| p.level).collect();
            project.edit(&levels, None, |layout| {
                for part in layout.parts.iter_mut().filter(|p| p.group == Some(g)) {
                    part.translate(delta);
                }
            });
        }
    }
    project.status = "Moved".into();
}

fn place(project: &mut Project, c: Candidate, item: Item) {
    let mut id = 0;
    project.edit(&[c.level], None, |layout| {
        id = layout.add_in(c.level, c.ground, c.group, c.kind)
    });
    project.selected = Some(id);
    project.selected_group = None;
    project.status = format!("Placed {}", item.label());
}

/// Terrain brushes: paint while the button is held; afterwards loose parts settle onto the new ground.
fn sculpt(
    buttons: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    mut project: ResMut<Project>,
    mut tool: ResMut<Tool>,
    pointer: Res<Pointer>,
) {
    let Some(Item::Terrain(kind)) = tool.item.filter(|_| project.mode == Mode::Build) else {
        return;
    };
    if buttons.just_released(MouseButton::Left) && tool.stroking {
        tool.stroking = false;
        tool.flatten_to = None;
        settle_loose_parts(&mut project);
        return;
    }
    let Some(p) = pointer.ground else { return };
    if buttons.just_pressed(MouseButton::Left) && !pointer.over_ui {
        tool.stroking = true;
        let bounds = project.layout.bounds().grow(40.0);
        project.edit(&[], None, |layout| {
            if layout.terrain.is_none() {
                layout.terrain = Some(Terrain::flat(
                    bounds,
                    (bounds.w.max(bounds.d) / 96.0).max(0.75),
                ));
            }
        });
        tool.flatten_to = Some(project.layout.ground_at(p));
    }
    if !tool.stroking || !buttons.pressed(MouseButton::Left) {
        return;
    }
    let dt = time.delta_secs_f64().min(0.1);
    let brush = match kind {
        Sculpt::Raise => Brush::Raise,
        Sculpt::Lower => Brush::Lower,
        Sculpt::Flatten => Brush::Flatten(tool.flatten_to.unwrap_or(0.0)),
        Sculpt::Smooth => Brush::Smooth,
    };
    let strength = match kind {
        Sculpt::Raise | Sculpt::Lower => tool.brush_strength * 3.0 * dt,
        _ => (tool.brush_strength * 4.0 * dt).min(1.0),
    };
    let radius = tool.brush_radius;
    if project
        .layout
        .terrain
        .as_mut()
        .is_some_and(|t| t.brush(brush, p, radius, strength))
    {
        project.mark_terrain();
    }
}

/// Puts trees, props and paths that belong to no building back onto the ground.
fn settle_loose_parts(project: &mut Project) {
    let Some(terrain) = project.layout.terrain.clone() else {
        return;
    };
    project.edit(&[0], Some("settle"), |layout| {
        for part in layout
            .parts
            .iter_mut()
            .filter(|p| p.group.is_none() && p.level == 0)
        {
            let c = part.footprint().center();
            part.ground = terrain.height_at([c.0, c.1]);
        }
    });
    project.mark_all();
}

#[allow(clippy::too_many_arguments)]
fn update_ghost(
    mut commands: Commands,
    project: Res<Project>,
    tool: Res<Tool>,
    pointer: Res<Pointer>,
    ghosts: Query<Entity, With<Ghost>>,
    materials: Res<GhostMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut shown: Local<Option<Candidate>>,
) {
    let wanted = if project.mode == Mode::Build && (!pointer.over_ui || tool.moving.is_some()) {
        candidate(&project, &tool, &pointer)
    } else {
        None
    };
    if *shown == wanted {
        return;
    }
    for entity in &ghosts {
        commands.entity(entity).despawn();
    }
    *shown = wanted.clone();
    let Some(c) = wanted else { return };
    let mut layout = Layout::new(
        "ghost",
        project.layout.style.clone(),
        project.layout.palette.clone(),
    );
    layout.add_in(c.level, c.ground, None, c.kind);
    let data = merged(&Scene::new(&layout).build_all(&View::default()));
    if data.is_empty() {
        return;
    }
    commands.spawn((
        Ghost,
        Mesh3d(meshes.add(bevy_mesh(&data))),
        MeshMaterial3d(if c.valid {
            materials.ok.clone()
        } else {
            materials.bad.clone()
        }),
        Transform::from_xyz(0.0, 0.01, 0.0),
    ));
}

fn draw_gizmos(mut gizmos: Gizmos, project: Res<Project>, tool: Res<Tool>, pointer: Res<Pointer>) {
    if project.mode != Mode::Build {
        return;
    }
    let lift = 0.04;
    if let Some(p) = pointer.ground {
        if let Some(Item::Terrain(_)) = tool.item {
            // The brush: a ring that follows the ground.
            let r = tool.brush_radius;
            let ring: Vec<Vec3> = (0..=48)
                .map(|k| {
                    let a = std::f64::consts::TAU * k as f64 / 48.0;
                    let q = [p[0] + r * a.cos(), p[1] + r * a.sin()];
                    v(q[0], project.layout.ground_at(q) + 0.08, q[1])
                })
                .collect();
            gizmos.linestrip(ring, Color::srgb(0.71, 0.32, 0.23));
        } else {
            // A fading patch of grid around the pointer, like a build-mode floor grid.
            let c = snap(p, false);
            let reach = 8;
            for i in -reach..=reach {
                let alpha = 0.35 * (1.0 - (i as f32).abs() / (reach as f32 + 1.0));
                let colour = Color::srgba(0.45, 0.33, 0.2, alpha);
                let o = i as f64 * GRID;
                let span = reach as f64 * GRID;
                let h = |x: f64, z: f64| surface_height(&project, [x, z]) + lift;
                gizmos.line(
                    v(c[0] + o, h(c[0] + o, c[1] - span), c[1] - span),
                    v(c[0] + o, h(c[0] + o, c[1] + span), c[1] + span),
                    colour,
                );
                gizmos.line(
                    v(c[0] - span, h(c[0] - span, c[1] + o), c[1] + o),
                    v(c[0] + span, h(c[0] + span, c[1] + o), c[1] + o),
                    colour,
                );
            }
            let marker = snap(p, pointer.free);
            gizmos.circle(
                Isometry3d::new(
                    v(
                        marker[0],
                        surface_height(&project, marker) + lift,
                        marker[1],
                    ),
                    Quat::from_rotation_x(FRAC_PI_2 as f32),
                ),
                0.12,
                Color::srgb(0.71, 0.32, 0.23),
            );
            if tool.item == Some(Item::Building) {
                let size = project.site.building.size;
                let r = Rect::new(
                    marker[0] - size.width / 2.0,
                    marker[1] - size.depth / 2.0,
                    size.width,
                    size.depth,
                );
                rect_outline(
                    &mut gizmos,
                    &r,
                    pointer.height + lift,
                    Color::srgb(0.55, 0.85, 0.45),
                );
            }
        }
    }
    let gold = Color::srgb(0.85, 0.66, 0.15);
    if let Some(r) = project
        .selected_group
        .and_then(|g| group_bounds(&project.layout, g))
    {
        let c = r.center();
        rect_outline(
            &mut gizmos,
            &r.grow(0.3),
            project.layout.ground_at([c.0, c.1]) + lift,
            gold,
        );
    }
    if let Some(part) = project.selected.and_then(|id| project.layout.get(id)) {
        let base = project.layout.base(part) + 0.05;
        match &part.kind {
            PartKind::Opening(o) => {
                let along = [o.facing[1], -o.facing[0]];
                let end =
                    |k: f64, h: f64| v(o.at[0] + along[0] * k, base + h, o.at[1] + along[1] * k);
                let (w, s, h) = (o.width / 2.0, o.sill, o.sill + o.height);
                for (a, b) in [
                    ((-w, s), (w, s)),
                    ((w, s), (w, h)),
                    ((w, h), (-w, h)),
                    ((-w, h), (-w, s)),
                ] {
                    gizmos.line(end(a.0, a.1), end(b.0, b.1), gold);
                }
            }
            _ => rect_outline(&mut gizmos, &part.footprint(), base, gold),
        }
    }
}

fn rect_outline(gizmos: &mut Gizmos, r: &Rect, y: f64, colour: Color) {
    let corners = [
        v(r.x, y, r.z),
        v(r.x1(), y, r.z),
        v(r.x1(), y, r.z1()),
        v(r.x, y, r.z1()),
    ];
    for i in 0..4 {
        gizmos.line(corners[i], corners[(i + 1) % 4], colour);
    }
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}
