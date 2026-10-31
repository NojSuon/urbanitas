//! The 3D view: textured materials, building meshes per level, camera and sunlight.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfigBuilder, GlobalAmbientLight};
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat};
use bevy::render::view::Msaa;
use bevy_panorbit_camera::PanOrbitCamera;
use urbanitas::mesh::build::{Scene, View};
use urbanitas::mesh::{Material, MeshData, MeshSet};
use urbanitas::style::Palette;
use urbanitas::texture;

use crate::project::{Mode, Project, Quality};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.80, 0.87, 0.93)))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.86, 0.89, 1.0),
                brightness: 900.0,
                affects_lightmapped_meshes: true,
            })
            .init_resource::<MaterialCache>()
            .init_resource::<CameraLook>()
            .init_resource::<SceneStats>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (track_camera_look, rebuild, level_visibility, apply_quality).chain(),
            );
    }
}

/// A mesh belonging to one level of the building.
#[derive(Component)]
pub struct LevelMesh {
    pub level: u32,
}

#[derive(Component)]
pub struct Sun;

/// Which way the camera looks, quantised to eight directions so cutaway redraws rarely.
#[derive(Resource, Default, PartialEq)]
pub struct CameraLook {
    pub octant: Option<i32>,
    pub dir: Option<[f64; 2]>,
}

#[derive(Resource, Default)]
pub struct SceneStats {
    per_level: HashMap<u32, usize>,
    pub build_ms: f64,
}

impl SceneStats {
    pub fn triangles(&self) -> usize {
        self.per_level.values().sum()
    }
}

#[derive(Resource, Default)]
pub struct MaterialCache {
    materials: HashMap<(Material, String, u32), Handle<StandardMaterial>>,
}

/// Only these materials take the palette's colours; the rest are shared by every building.
fn palette_key(material: Material, palette: &Palette) -> String {
    match material {
        Material::Plaster
        | Material::Rustication
        | Material::Stone
        | Material::Shutter
        | Material::Door
        | Material::RoofTile => {
            format!(
                "{:?}",
                (palette.walls, palette.trim, palette.roof, palette.stone)
            )
        }
        _ => String::new(),
    }
}

impl MaterialCache {
    pub fn get(
        &mut self,
        material: Material,
        palette: &Palette,
        quality: Quality,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        let size = quality.texture_size();
        let key = (material, palette_key(material, palette), size);
        self.materials
            .entry(key)
            .or_insert_with(|| {
                let tex = texture::generate(material, palette, size);
                let image = images.add(make_image(&tex, quality));
                let (roughness, metallic) = material.surface();
                materials.add(StandardMaterial {
                    base_color_texture: Some(image),
                    perceptual_roughness: roughness,
                    metallic,
                    reflectance: if material == Material::Glass {
                        0.6
                    } else {
                        0.3
                    },
                    ..default()
                })
            })
            .clone()
    }
}

fn make_image(tex: &texture::Texture, quality: Quality) -> Image {
    let size = Extent3d {
        width: tex.size,
        height: tex.size,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_uninit(
        size,
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(tex.mip_bytes());
    image.texture_descriptor.mip_level_count = tex.levels.len() as u32;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: if quality == Quality::High { 8 } else { 1 },
        ..default()
    });
    image
}

/// Converts a core mesh to a Bevy mesh. Core UVs grow upwards; Bevy's grow downwards.
pub fn bevy_mesh(data: &MeshData) -> Mesh {
    let uvs: Vec<[f32; 2]> = data.uvs.iter().map(|&[u, v]| [u, -v]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, data.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(data.indices.clone()))
}

/// All of a mesh set's materials merged into one mesh (for the placement ghost).
pub fn merged(set: &MeshSet) -> MeshData {
    let mut out = MeshData::default();
    for mesh in set.meshes.values() {
        out.append(mesh);
    }
    out
}

/// Where the camera starts: angles in radians and a multiplier on the framing distance.
#[derive(Resource, Clone, Copy)]
pub struct CameraStart {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
}

impl Default for CameraStart {
    fn default() -> Self {
        // Yaw 0 looks from the north; start from the street (south) side.
        Self {
            yaw: 2.5,
            pitch: 0.45,
            zoom: 1.0,
        }
    }
}

fn setup(mut commands: Commands, project: Res<Project>, start: Option<Res<CameraStart>>) {
    let start = start.map(|s| *s).unwrap_or_default();
    let bounds = project.layout.bounds();
    let (cx, cz) = bounds.center();
    let radius = (bounds.w.max(bounds.d) * 1.25).max(18.0) as f32 * start.zoom;
    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        PanOrbitCamera {
            focus: Vec3::new(cx as f32, 3.0, cz as f32),
            radius: Some(radius),
            yaw: Some(start.yaw),
            pitch: Some(start.pitch),
            button_orbit: MouseButton::Right,
            button_pan: MouseButton::Middle,
            pitch_lower_limit: Some(0.05),
            zoom_lower_limit: 3.0,
            zoom_upper_limit: Some(250.0),
            ..default()
        },
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            color: Color::srgb(1.0, 0.93, 0.82),
            illuminance: 9_000.0,
            shadow_maps_enabled: project.quality == Quality::High,
            ..default()
        },
        // Afternoon sun from the south-east, lighting the street front.
        Transform::from_xyz(25.0, 45.0, -35.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 2,
            first_cascade_far_bound: 25.0,
            maximum_distance: 120.0,
            ..default()
        }
        .build(),
    ));
    // A soft, shadowless sky bounce from the other side so no facade goes flat.
    commands.spawn((
        DirectionalLight {
            color: Color::srgb(0.78, 0.85, 1.0),
            illuminance: 2_200.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-30.0, 20.0, 30.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn track_camera_look(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut look: ResMut<CameraLook>,
    mut project: ResMut<Project>,
) {
    let Ok(transform) = camera.single() else {
        return;
    };
    let forward = transform.forward();
    let flat = Vec2::new(forward.x, forward.z);
    if flat.length() < 1e-3 {
        return;
    }
    let octant =
        ((flat.y.atan2(flat.x) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
    if look.octant != Some(octant) {
        let angle = octant as f64 * std::f64::consts::FRAC_PI_4;
        look.octant = Some(octant);
        look.dir = Some([angle.cos(), angle.sin()]);
        if project.mode == Mode::Build && project.walls == urbanitas::mesh::build::WallMode::Cutaway
        {
            let level = project.level;
            project.mark(&[level]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    mut project: ResMut<Project>,
    mut cache: ResMut<MaterialCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<(Entity, &LevelMesh)>,
    look: Res<CameraLook>,
    mut stats: ResMut<SceneStats>,
) {
    let Some(dirty) = project.take_dirty() else {
        return;
    };
    let started = std::time::Instant::now();
    let top = project.top_level();
    let levels: Vec<u32> = dirty.clone().unwrap_or_else(|| (0..=top).collect());
    for (entity, mesh) in &existing {
        if dirty.is_none() || levels.contains(&mesh.level) {
            commands.entity(entity).despawn();
        }
    }
    if dirty.is_none() {
        stats.per_level.clear();
    }
    let scene = Scene::new(&project.layout);
    for &level in &levels {
        let view = if project.mode == Mode::Build && level == project.level {
            View {
                walls: project.walls,
                look: look.dir,
            }
        } else {
            View::default()
        };
        let set = scene.build_level(level, &view);
        stats.per_level.insert(level, set.triangle_count());
        for (material, data) in &set.meshes {
            if data.is_empty() {
                continue;
            }
            let handle = cache.get(
                *material,
                &project.layout.palette,
                project.quality,
                &mut images,
                &mut materials,
            );
            commands.spawn((
                Mesh3d(meshes.add(bevy_mesh(data))),
                MeshMaterial3d(handle),
                Transform::default(),
                LevelMesh { level },
            ));
        }
    }
    stats.build_ms = started.elapsed().as_secs_f64() * 1000.0;
}

fn level_visibility(project: Res<Project>, mut meshes: Query<(&LevelMesh, &mut Visibility)>) {
    for (mesh, mut visibility) in &mut meshes {
        let show = project.mode == Mode::Generate || mesh.level <= project.level;
        let wanted = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

fn apply_quality(
    project: Res<Project>,
    mut cameras: Query<&mut Msaa, With<Camera3d>>,
    mut suns: Query<&mut DirectionalLight, With<Sun>>,
    mut last: Local<Option<Quality>>,
) {
    if *last == Some(project.quality) {
        return;
    }
    *last = Some(project.quality);
    for mut msaa in &mut cameras {
        *msaa = if project.quality == Quality::High {
            Msaa::Sample4
        } else {
            Msaa::Off
        };
    }
    for mut sun in &mut suns {
        sun.shadow_maps_enabled = project.quality == Quality::High;
    }
}
