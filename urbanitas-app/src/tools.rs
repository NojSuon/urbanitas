//! Build mode: the parts catalogue, Sims-style placement with grid snapping (or free
//! placement while Alt is held), selection, moving, rotating, deleting and the ghost preview.

use std::f64::consts::{FRAC_PI_2, PI};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use urbanitas::geometry::Rect;
use urbanitas::mesh::build::{Scene, View, WallMode};
use urbanitas::model::{OpeningKind, Roof};
use urbanitas::parts::*;
use urbanitas::style::OpeningSpec;

use crate::project::{Mode, Project};
use crate::scene::{bevy_mesh, merged};

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
                (track_pointer, keyboard, mouse, update_ghost, draw_gizmos).chain(),
            );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Walls,
    Openings,
    Structure,
    Roofs,
    Garden,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Walls,
        Category::Openings,
        Category::Structure,
        Category::Roofs,
        Category::Garden,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Walls => "Walls",
            Category::Openings => "Openings",
            Category::Structure => "Structure",
            Category::Roofs => "Roofs",
            Category::Garden => "Garden",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    FacadeWall,
    Partition,
    ArchedWindow,
    SquareWindow,
    SmallWindow,
    FrontDoor,
    BalconyDoor,
    Storefront,
    Archway,
    Balcony,
    Floor,
    Stairs,
    HipRoof,
    FlatRoof,
    Area(AreaKind),
    Prop(PropKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    /// Click and drag from one end to the other.
    Line,
    /// Click and drag from corner to corner.
    Rect,
    /// Click on a wall.
    Mounted,
    /// Click anywhere.
    Point,
}

impl Item {
    pub fn catalogue(category: Category) -> Vec<Item> {
        match category {
            Category::Walls => vec![Item::FacadeWall, Item::Partition],
            Category::Openings => vec![
                Item::ArchedWindow,
                Item::SquareWindow,
                Item::SmallWindow,
                Item::FrontDoor,
                Item::BalconyDoor,
                Item::Storefront,
                Item::Archway,
                Item::Balcony,
            ],
            Category::Structure => vec![
                Item::Floor,
                Item::Stairs,
                Item::Prop(PropKind::Column),
                Item::Area(AreaKind::Loggia),
            ],
            Category::Roofs => vec![Item::HipRoof, Item::FlatRoof],
            Category::Garden => {
                let mut items: Vec<Item> = [
                    AreaKind::Lawn,
                    AreaKind::Path,
                    AreaKind::Terrace,
                    AreaKind::Pool,
                    AreaKind::Pergola,
                    AreaKind::Hedge,
                ]
                .into_iter()
                .map(Item::Area)
                .collect();
                items.extend(
                    PropKind::ALL
                        .into_iter()
                        .filter(|k| *k != PropKind::Column)
                        .map(Item::Prop),
                );
                items
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Item::FacadeWall => "Facade wall",
            Item::Partition => "Partition",
            Item::ArchedWindow => "Arched window",
            Item::SquareWindow => "Square window",
            Item::SmallWindow => "Small window",
            Item::FrontDoor => "Front door",
            Item::BalconyDoor => "Balcony door",
            Item::Storefront => "Storefront",
            Item::Archway => "Archway",
            Item::Balcony => "Balcony",
            Item::Floor => "Floor",
            Item::Stairs => "Stairs",
            Item::HipRoof => "Hipped roof",
            Item::FlatRoof => "Roof terrace",
            Item::Area(kind) => kind.label(),
            Item::Prop(kind) => kind.label(),
        }
    }

    fn placement(self) -> Placement {
        match self {
            Item::FacadeWall | Item::Partition => Placement::Line,
            Item::Floor | Item::HipRoof | Item::FlatRoof | Item::Area(_) => Placement::Rect,
            Item::ArchedWindow
            | Item::SquareWindow
            | Item::SmallWindow
            | Item::FrontDoor
            | Item::BalconyDoor
            | Item::Storefront
            | Item::Archway
            | Item::Balcony => Placement::Mounted,
            Item::Stairs | Item::Prop(_) => Placement::Point,
        }
    }

    pub fn hint(self) -> &'static str {
        match self.placement() {
            Placement::Line => "Drag along the grid to draw a wall",
            Placement::Rect => "Drag from corner to corner",
            Placement::Mounted => "Point at a wall and click",
            Placement::Point => "Click to place · R to turn",
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
    drag_start: Option<P2>,
    /// The part being dragged and where it was grabbed.
    moving: Option<(PartId, P2)>,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            item: None,
            category: Category::Walls,
            yaw: 0.0,
            heading: Heading::North,
            drag_start: None,
            moving: None,
        }
    }
}

impl Tool {
    pub fn pick(&mut self, item: Option<Item>) {
        self.item = item;
        self.drag_start = None;
        self.moving = None;
    }
}

/// Where the pointer meets the active level's floor.
#[derive(Resource, Default)]
pub struct Pointer {
    pub ground: Option<P2>,
    /// Alt held: no snapping, fine rotation.
    pub free: bool,
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

/// Height of the floor surface of the active level.
pub fn floor_height(project: &Project) -> f64 {
    let level = project.level;
    project.layout.elevation(level)
        + if level > 0 {
            project.layout.style.slab_thickness
        } else {
            0.0
        }
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

/// Snaps to a nearby wall end so new walls join old ones cleanly; otherwise to the grid.
fn snap_wall_end(layout: &Layout, level: u32, p: P2, free: bool) -> P2 {
    walls_on(layout, level)
        .flat_map(|(_, w)| [w.a, w.b])
        .map(|q| (q, dist(p, q)))
        .filter(|(_, d)| *d < 0.35)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or_else(|| snap(p, free), |(q, _)| q)
}

fn level_centre(layout: &Layout, level: u32) -> P2 {
    let floors: Vec<Rect> = layout
        .on_level(level)
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

/// What the current tool would place at the pointer: `(level, part, valid)`.
fn candidate(project: &Project, tool: &Tool, pointer: &Pointer) -> Option<(u32, PartKind, bool)> {
    let layout = &project.layout;
    let style = &layout.style;
    let level = project.level;
    let raw = pointer.ground?;
    let free = pointer.free;

    if let Some((id, grab)) = tool.moving {
        let part = layout.get(id)?;
        let mut moved = part.clone();
        let (from, to) = (snap(grab, free), snap(raw, free));
        moved.translate([to[0] - from[0], to[1] - from[1]]);
        remount(layout, &mut moved);
        return Some((moved.level, moved.kind, true));
    }

    let item = tool.item?;
    match item.placement() {
        Placement::Line => {
            let start = tool.drag_start?;
            let end = snap_wall_end(layout, level, raw, free);
            let exterior = item == Item::FacadeWall;
            let wall = Wall {
                a: start,
                b: end,
                height: style.floor_height,
                thickness: if exterior {
                    style.wall_thickness
                } else {
                    style.partition_thickness
                },
                exterior,
            };
            Some((level, PartKind::Wall(wall), wall.length() >= 0.5))
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
                Item::HipRoof => {
                    let overhang = style.eave_overhang;
                    let short = rect.w.min(rect.d) + 2.0 * overhang;
                    let pitch = style.roof_pitch_deg;
                    let ridge_height = short / 2.0 * pitch.to_radians().tan();
                    PartKind::Roof(RoofPart {
                        rect,
                        roof: Roof::Hip {
                            pitch_deg: pitch,
                            overhang,
                            ridge_height,
                        },
                    })
                }
                Item::FlatRoof => PartKind::Roof(RoofPart {
                    rect,
                    roof: Roof::Flat {
                        parapet_height: style.parapet_height,
                    },
                }),
                Item::Area(kind) => {
                    let height = if kind == AreaKind::Loggia {
                        style.floor_height
                    } else {
                        kind.default_height()
                    };
                    PartKind::Area(AreaPart { kind, rect, height })
                }
                _ => return None,
            };
            Some((level, kind, rect.w >= 0.5 && rect.d >= 0.5))
        }
        Placement::Mounted => mounted(project, item, raw, free).map(|(kind, ok)| (level, kind, ok)),
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
            Some((level, kind, true))
        }
    }
}

/// An opening or balcony on the wall nearest the pointer, kept inside the wall and clear of
/// other openings.
fn mounted(project: &Project, item: Item, p: P2, free: bool) -> Option<(PartKind, bool)> {
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
        let c = level_centre(layout, level);
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
        if layout.host_wall(level, o_at).map(|(h, _)| h.id) != Some(host.id) {
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
        ));
    }
    let sill = spec.sill + style.slab_thickness;
    valid &= sill + spec.height <= wall.height - 0.05;
    let kind = match item {
        Item::FrontDoor => OpeningKind::Door,
        Item::BalconyDoor => OpeningKind::BalconyDoor,
        Item::Storefront => OpeningKind::Storefront,
        Item::Archway => OpeningKind::Archway,
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
    Some((PartKind::Opening(opening), valid))
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
        PartKind::Area(a) if a.kind != AreaKind::Lawn => 5,
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

fn track_pointer(
    mut pointer: ResMut<Pointer>,
    project: Res<Project>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    pointer.free = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if let Some(forced) = pointer.forced {
        pointer.ground = Some(forced);
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
    let plane = floor_height(&project) as f32;
    if ray.direction.y.abs() < 1e-4 {
        return;
    }
    let t = (plane - ray.origin.y) / ray.direction.y;
    if t > 0.0 {
        let hit = ray.origin + *ray.direction * t;
        pointer.ground = Some([hit.x as f64, hit.z as f64]);
    }
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
        if let Some(id) = project.selected {
            delete_part(&mut project, id);
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        tool.pick(None);
        project.selected = None;
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
            project.selected = hit;
            if let Some(id) = hit {
                tool.moving = Some((id, p));
                project.status = project.layout.get(id).map(Part::label).unwrap_or_default();
            }
        }
        if released {
            if let Some((id, grab)) = tool.moving.take() {
                let (from, to) = (snap(grab, pointer.free), snap(p, pointer.free));
                let delta = [to[0] - from[0], to[1] - from[1]];
                if delta[0].abs() + delta[1].abs() > 1e-6 {
                    let level = project.layout.get(id).map_or(0, |part| part.level);
                    project.edit(&[level], None, |layout| {
                        let snapshot = layout.clone();
                        if let Some(part) = layout.get_mut(id) {
                            part.translate(delta);
                            remount(&snapshot, part);
                        }
                    });
                    project.status = "Moved".into();
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
                if let Some((level, kind, true)) = candidate(&project, &tool, &pointer) {
                    place(&mut project, level, kind, item);
                }
                tool.drag_start = None;
            }
        }
        Placement::Mounted | Placement::Point => {
            if pressed {
                match candidate(&project, &tool, &pointer) {
                    Some((level, kind, true)) => place(&mut project, level, kind, item),
                    Some(_) => project.status = "That does not fit there".into(),
                    None => project.status = "Point at a wall".into(),
                }
            }
        }
    }
}

fn place(project: &mut Project, level: u32, kind: PartKind, item: Item) {
    let mut id = 0;
    project.edit(&[level], None, |layout| id = layout.add(level, kind));
    project.selected = Some(id);
    project.status = format!("Placed {}", item.label());
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
    mut shown: Local<Option<(u32, PartKind, bool)>>,
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
    let Some((level, kind, valid)) = wanted else {
        return;
    };
    let mut layout = Layout::new(
        "ghost",
        project.layout.style.clone(),
        project.layout.palette.clone(),
    );
    layout.add(level, kind);
    let data = merged(&Scene::new(&layout).build_all(&View::default()));
    if data.is_empty() {
        return;
    }
    commands.spawn((
        Ghost,
        Mesh3d(meshes.add(bevy_mesh(&data))),
        MeshMaterial3d(if valid {
            materials.ok.clone()
        } else {
            materials.bad.clone()
        }),
        Transform::from_xyz(0.0, 0.01, 0.0),
    ));
}

fn draw_gizmos(mut gizmos: Gizmos, project: Res<Project>, pointer: Res<Pointer>) {
    if project.mode != Mode::Build {
        return;
    }
    let y = floor_height(&project) + 0.03;
    if let Some(p) = pointer.ground {
        // A fading patch of grid around the pointer, like a build-mode floor grid.
        let c = snap(p, false);
        let reach = 8;
        for i in -reach..=reach {
            let alpha = 0.35 * (1.0 - (i as f32).abs() / (reach as f32 + 1.0));
            let colour = Color::srgba(0.45, 0.33, 0.2, alpha);
            let o = i as f64 * GRID;
            let span = reach as f64 * GRID;
            gizmos.line(
                v(c[0] + o, y, c[1] - span),
                v(c[0] + o, y, c[1] + span),
                colour,
            );
            gizmos.line(
                v(c[0] - span, y, c[1] + o),
                v(c[0] + span, y, c[1] + o),
                colour,
            );
        }
        let marker = snap(p, pointer.free);
        gizmos.circle(
            Isometry3d::new(
                v(marker[0], y, marker[1]),
                Quat::from_rotation_x(FRAC_PI_2 as f32),
            ),
            0.12,
            Color::srgb(0.71, 0.32, 0.23),
        );
    }
    if let Some(part) = project.selected.and_then(|id| project.layout.get(id)) {
        let gold = Color::srgb(0.85, 0.66, 0.15);
        let base = project.layout.elevation(part.level) + 0.05;
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
            _ => {
                let r = part.footprint();
                let corners = [
                    v(r.x, base, r.z),
                    v(r.x1(), base, r.z),
                    v(r.x1(), base, r.z1()),
                    v(r.x, base, r.z1()),
                ];
                for i in 0..4 {
                    gizmos.line(corners[i], corners[(i + 1) % 4], gold);
                }
            }
        }
    }
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}
