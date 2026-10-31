//! Pictures of parts: a small render of every catalogue item, and a larger one of whatever
//! is selected, painted off-screen one at a time and shown in the HUD.

use std::collections::{HashMap, VecDeque};

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::view::Msaa;
use bevy_egui::{EguiTextureHandle, EguiUserTextures, egui};
use urbanitas::geometry::Rect;
use urbanitas::mesh::build::{Scene, View};
use urbanitas::model::{OpeningKind, Roof};
use urbanitas::parts::*;
use urbanitas::style::OpeningSpec;
use urbanitas::terrain::{Brush, Terrain};
use urbanitas::{BuildingParams, Garden, Size, generate};

use crate::project::Project;
use crate::scene::{MaterialCache, bevy_mesh};
use crate::tools::{Item, Sculpt, Tool};

/// The render layer the picture cameras and models live on, away from the main view.
const LAYER: usize = 7;
const THUMB: u32 = 112;
const PREVIEW: (u32, u32) = (264, 168);
/// Frames to keep a job on screen before reading it as done.
const FRAMES: u32 = 3;

pub struct ThumbsPlugin;

impl Plugin for ThumbsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Thumbs>()
            .add_systems(Startup, setup)
            .add_systems(PostUpdate, run);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Item(Item),
    Preview,
}

#[derive(Resource, Default)]
pub struct Thumbs {
    ready: HashMap<Key, egui::TextureId>,
    queue: VecDeque<Key>,
    active: Option<Job>,
    /// What the pictures were painted for: style and palette.
    painted_for: Option<String>,
    /// What the preview shows, and when it was last painted.
    preview_of: Option<(Option<PartId>, Option<u32>, u64)>,
}

struct Job {
    key: Key,
    entities: Vec<Entity>,
    image: Handle<Image>,
    frames: u32,
}

impl Thumbs {
    pub fn item(&self, item: Item) -> Option<egui::TextureId> {
        self.ready.get(&Key::Item(item)).copied()
    }

    pub fn preview(&self) -> Option<egui::TextureId> {
        self.ready.get(&Key::Preview).copied()
    }
}

#[derive(Component)]
struct PictureLight;

fn setup(mut commands: Commands) {
    commands.spawn((
        PictureLight,
        DirectionalLight {
            illuminance: 9_000.0,
            color: Color::srgb(1.0, 0.95, 0.86),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(20.0, 40.0, -30.0).looking_at(Vec3::ZERO, Vec3::Y),
        RenderLayers::layer(LAYER),
    ));
}

fn target(size: (u32, u32)) -> Image {
    let extent = Extent3d {
        width: size.0,
        height: size.1,
        ..default()
    };
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("urbanitas picture"),
            size: extent,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8UnormSrgb,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(extent);
    image
}

/// A small scene showing one catalogue item, in the project's style and colours.
fn sample(item: Item, project: &Project) -> Layout {
    let style = project.layout.style.clone();
    let palette = project
        .layout
        .palette_of(project.selected_group.or(Some(0)))
        .clone();
    let mut layout = Layout::new("picture", style.clone(), palette);
    let wall = |finish: WallFinish, exterior: bool| Wall {
        a: [-2.5, 0.0],
        b: [2.5, 0.0],
        height: if finish == WallFinish::Building {
            style.floor_height
        } else {
            finish.default_height()
        },
        thickness: if finish == WallFinish::Building && exterior {
            style.wall_thickness
        } else {
            finish.default_thickness()
        },
        exterior,
        base: 0.0,
        finish,
    };
    let mount = |layout: &mut Layout, kind: OpeningKind, spec: OpeningSpec| {
        layout.add(0, PartKind::Wall(wall(WallFinish::Building, true)));
        layout.add(
            0,
            PartKind::Opening(OpeningPart {
                kind,
                at: [0.0, 0.0],
                width: spec.width,
                height: spec.height,
                sill: spec.sill + style.slab_thickness,
                arched: spec.arched,
                facing: [0.0, -1.0],
            }),
        );
    };
    let rect = |w: f64, d: f64| Rect::new(-w / 2.0, -d / 2.0, w, d);
    match item {
        Item::Wall(finish, exterior) => {
            layout.add(0, PartKind::Wall(wall(finish, exterior)));
        }
        Item::ArchedWindow => mount(&mut layout, OpeningKind::Window, style.window),
        Item::SquareWindow => mount(
            &mut layout,
            OpeningKind::Window,
            OpeningSpec {
                arched: false,
                ..style.window
            },
        ),
        Item::SmallWindow => mount(&mut layout, OpeningKind::Window, style.small_window),
        Item::FrontDoor => mount(&mut layout, OpeningKind::Door, style.entrance),
        Item::BalconyDoor => mount(&mut layout, OpeningKind::BalconyDoor, style.balcony_door),
        Item::Storefront => mount(&mut layout, OpeningKind::Storefront, style.storefront),
        Item::Archway => mount(&mut layout, OpeningKind::Archway, style.interior_door),
        Item::Gate => {
            layout.add(
                0,
                PartKind::Wall(Wall {
                    a: [-3.5, 0.0],
                    b: [3.5, 0.0],
                    ..wall(WallFinish::Garden, false)
                }),
            );
            let h = WallFinish::Garden.default_height();
            layout.add(
                0,
                PartKind::Opening(OpeningPart {
                    kind: OpeningKind::Gate,
                    at: [0.0, 0.0],
                    width: 2.4,
                    height: h,
                    sill: 0.0,
                    arched: false,
                    facing: [0.0, -1.0],
                }),
            );
        }
        Item::Balcony => {
            layout.add(1, PartKind::Wall(wall(WallFinish::Building, true)));
            layout.add(
                1,
                PartKind::Balcony(BalconyPart {
                    at: [0.0, 0.0],
                    width: 2.4,
                    depth: style.balcony_depth,
                    facing: [0.0, -1.0],
                }),
            );
        }
        Item::Floor => {
            layout.add(
                0,
                PartKind::Floor(FloorPart {
                    rect: rect(4.0, 3.0),
                    room: None,
                }),
            );
        }
        Item::Stairs => {
            layout.add(
                0,
                PartKind::Stairs(StairsPart {
                    rect: rect(2.4, 4.8),
                    rise: style.floor_height,
                    heading: Heading::North,
                }),
            );
        }
        Item::HipRoof => {
            let r = rect(8.0, 6.0);
            layout.add(
                0,
                PartKind::Roof(RoofPart {
                    rect: r,
                    roof: Roof::hip(&r, style.roof_pitch_deg, style.eave_overhang),
                    open: [false; 4],
                }),
            );
        }
        Item::FlatRoof => {
            layout.add(
                0,
                PartKind::Roof(RoofPart {
                    rect: rect(8.0, 6.0),
                    roof: Roof::Flat {
                        parapet_height: style.parapet_height,
                        balustrade: style.roof_balustrade,
                    },
                    open: [false; 4],
                }),
            );
        }
        Item::Area(kind) => {
            let r = match kind {
                AreaKind::Courtyard => rect(11.0, 11.0),
                AreaKind::Tower => rect(3.4, 3.4),
                AreaKind::Loggia => rect(7.0, 3.0),
                AreaKind::Street | AreaKind::Road => rect(7.0, 4.0),
                _ => rect(6.0, 4.0),
            };
            let height = match kind {
                AreaKind::Loggia | AreaKind::Courtyard => style.floor_height,
                AreaKind::Platform => 1.0,
                k => k.default_height(),
            };
            layout.add(
                0,
                PartKind::Area(AreaPart {
                    kind,
                    rect: r,
                    height,
                }),
            );
        }
        Item::Prop(kind) => {
            layout.add(
                0,
                PartKind::Prop(PropPart {
                    kind,
                    at: [0.0, 0.0],
                    yaw: 0.0,
                    scale: 1.0,
                    height: kind.default_height(),
                }),
            );
        }
        Item::Terrain(sculpt) => {
            let mut t = Terrain::flat(rect(16.0, 16.0), 0.5);
            let brush = match sculpt {
                Sculpt::Raise => Brush::Raise,
                Sculpt::Lower => Brush::Lower,
                Sculpt::Flatten => Brush::Flatten(0.0),
                Sculpt::Smooth => Brush::Smooth,
            };
            if matches!(sculpt, Sculpt::Flatten | Sculpt::Smooth) {
                t = Terrain::generate(rect(16.0, 16.0), 0.5, 1.5, 0.0, 3);
            }
            t.brush(
                brush,
                [0.0, 0.0],
                5.5,
                if matches!(sculpt, Sculpt::Raise | Sculpt::Lower) {
                    2.2
                } else {
                    1.0
                },
            );
            layout.terrain = Some(t);
        }
        Item::Building => {
            let params = BuildingParams {
                size: Size {
                    width: project.site.building.size.width.min(14.0),
                    depth: project.site.building.size.depth.min(12.0),
                },
                rooms: project.site.building.rooms.min(4),
                garden: Garden::None,
                ..project.site.building.clone()
            };
            if let Ok(b) = generate(&params) {
                layout = b.to_layout();
            }
        }
    }
    layout
}

/// The selected part (with its wall for openings), or every part of the selected building.
fn selection(project: &Project) -> Option<Layout> {
    let layout = &project.layout;
    let mut out = Layout::new("preview", layout.style.clone(), layout.palette.clone());
    out.palettes = layout.palettes.clone();
    if let Some(g) = project.selected_group {
        for part in layout.group_parts(g) {
            out.restore(Part {
                ground: 0.0,
                ..part.clone()
            });
        }
        return Some(out);
    }
    let part = layout.get(project.selected?)?;
    let at = match &part.kind {
        PartKind::Opening(o) => Some(o.at),
        PartKind::Balcony(b) => Some(b.at),
        _ => None,
    };
    if let Some((host, _)) = at.and_then(|at| layout.host_wall_in(part.level, part.group, at)) {
        out.restore(Part {
            ground: 0.0,
            ..host.clone()
        });
    }
    out.restore(Part {
        ground: 0.0,
        ..part.clone()
    });
    Some(out)
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut commands: Commands,
    mut thumbs: ResMut<Thumbs>,
    project: Res<Project>,
    mut cache: ResMut<MaterialCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut user_textures: ResMut<EguiUserTextures>,
    tool: Res<Tool>,
) {
    // Repaint everything when the style or colours change.
    let style_key = format!(
        "{:?}{:?}",
        project.layout.style.style,
        project.layout.palette_of(Some(0)).name
    );
    if thumbs.painted_for.as_ref() != Some(&style_key) {
        thumbs.painted_for = Some(style_key);
        let all: Vec<Key> = Item::all().into_iter().map(Key::Item).collect();
        thumbs.queue = all.into();
    }
    // The preview follows the selection.
    let wanted = (project.selected, project.selected_group, project.revision);
    let has_selection = project.selected.is_some() || project.selected_group.is_some();
    if has_selection && thumbs.preview_of != Some(wanted) && !thumbs.queue.contains(&Key::Preview) {
        thumbs.preview_of = Some(wanted);
        thumbs.queue.push_front(Key::Preview);
    }
    if !has_selection {
        thumbs.preview_of = None;
        thumbs.ready.remove(&Key::Preview);
    }

    if let Some(job) = thumbs.active.as_mut() {
        job.frames += 1;
        if job.frames < FRAMES {
            return;
        }
        let job = thumbs.active.take().expect("an active job");
        for entity in job.entities {
            commands.entity(entity).despawn();
        }
        let id = user_textures.add_image(EguiTextureHandle::Strong(job.image));
        thumbs.ready.insert(job.key, id);
    }

    // Paint the category on show first.
    let showing = Item::catalogue(tool.category);
    if let Some(i) = thumbs
        .queue
        .iter()
        .position(|k| matches!(k, Key::Item(item) if showing.contains(item)))
    {
        if !matches!(thumbs.queue.front(), Some(Key::Preview)) {
            let key = thumbs.queue.remove(i).expect("index from position");
            thumbs.queue.push_front(key);
        }
    }
    let Some(key) = thumbs.queue.pop_front() else {
        return;
    };
    let layout = match key {
        Key::Item(item) => sample(item, &project),
        Key::Preview => match selection(&project) {
            Some(l) => l,
            None => return,
        },
    };
    let size = match key {
        Key::Item(_) => (THUMB, THUMB),
        Key::Preview => PREVIEW,
    };
    let image = images.add(target(size));
    let layer = RenderLayers::layer(LAYER);
    let scene = Scene::new(&layout);
    let mut entities = Vec::new();
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let top = layout.parts.iter().map(|p| p.level).max().unwrap_or(0);
    let mut sets = Vec::new();
    for level in 0..=top {
        sets.extend(scene.build_level_groups(level, &View::default()));
    }
    sets.push((None, scene.build_terrain()));
    for (group, set) in sets {
        let palette = layout.palette_of(group).clone();
        for (material, data) in &set.meshes {
            if data.is_empty() {
                continue;
            }
            for p in &data.positions {
                lo = lo.min(Vec3::from_array(*p));
                hi = hi.max(Vec3::from_array(*p));
            }
            let handle = cache.get(
                *material,
                &palette,
                project.quality,
                &mut images,
                &mut materials,
            );
            entities.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(bevy_mesh(data))),
                        MeshMaterial3d(handle),
                        Transform::default(),
                        layer.clone(),
                    ))
                    .id(),
            );
        }
    }
    if entities.is_empty() {
        return;
    }
    let centre = (lo + hi) / 2.0;
    let radius = ((hi - lo).length() / 2.0).max(0.5);
    let eye = centre + Vec3::new(-0.62, 0.55, -1.0).normalize() * radius * 2.3;
    entities.push(
        commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -1,
                    clear_color: ClearColorConfig::Custom(Color::srgb(0.95, 0.92, 0.85)),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Msaa::Off,
                Transform::from_translation(eye).looking_at(centre, Vec3::Y),
                layer,
            ))
            .id(),
    );
    thumbs.active = Some(Job {
        key,
        entities,
        image,
        frames: 0,
    });
}
