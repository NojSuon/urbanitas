//! Urbanitas, the interactive app: generate a building or a whole neighbourhood live from
//! parameters, then edit it part by part in a Sims-style build mode. By Noj Suon.

mod capture;
mod files;
mod hud;
mod project;
mod scene;
mod theme;
mod thumbs;
mod tools;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::EguiPlugin;
use bevy_panorbit_camera::PanOrbitCameraPlugin;
use clap::{Parser, ValueEnum};
use urbanitas::mesh::build::WallMode;
use urbanitas::parts::{OpeningPart, PartKind};
use urbanitas::{
    ArchitectureStyle, BuildingParams, BuildingType, Context, Garden, Shape, SiteParams, Size,
};

use project::{Mode, Project, Quality};
use tools::{Category, Item, Pointer, Sculpt, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum QualityArg {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Demo {
    /// Build mode with the cutaway view, a window selected and an olive tree being placed.
    Build,
    /// Build mode with the terrain brushes and a hill raised next to the buildings.
    Terrain,
}

/// Urbanitas: procedural architecture you can edit.
#[derive(Parser, Debug)]
#[command(name = "urbanitas-app", version, author = "Noj Suon")]
struct Args {
    #[arg(long, value_enum, default_value_t = Context::Single)]
    context: Context,
    /// Buildings in an urban or suburban site.
    #[arg(long, default_value_t = 6)]
    buildings: u32,
    /// Height of the hills, in metres.
    #[arg(long, default_value_t = 0.0)]
    relief: f64,
    #[arg(long, value_enum, default_value_t = ArchitectureStyle::Florentine)]
    style: ArchitectureStyle,
    #[arg(long = "type", value_enum, default_value_t = BuildingType::Villa)]
    building_type: BuildingType,
    #[arg(long, value_enum, default_value_t = Shape::Auto)]
    shape: Shape,
    #[arg(long, default_value_t = 2)]
    floors: u32,
    #[arg(long, default_value_t = 18.0)]
    width: f64,
    #[arg(long, default_value_t = 16.0)]
    depth: f64,
    #[arg(long, default_value_t = 5)]
    rooms: u32,
    #[arg(long, value_enum, default_value_t = Garden::Surrounding)]
    garden: Garden,
    #[arg(long, default_value_t = 7)]
    seed: u64,
    /// Low turns off shadows and anti-aliasing and halves texture sizes, for low-end PCs.
    #[arg(long, value_enum, default_value_t = QualityArg::High)]
    quality: QualityArg,
    /// Render until the scene settles, save a PNG here and quit.
    #[arg(long, value_name = "FILE")]
    screenshot: Option<PathBuf>,
    /// Seconds the scene must stay still before the screenshot (shader compilation can be slow).
    #[arg(long, default_value_t = 20.0)]
    settle: f32,
    /// Start from a scripted scene.
    #[arg(long, value_enum)]
    demo: Option<Demo>,
    /// Camera yaw in degrees; 180 looks at the street front.
    #[arg(long, default_value_t = 145.0, allow_hyphen_values = true)]
    yaw: f32,
    /// Camera pitch in degrees.
    #[arg(long, default_value_t = 26.0)]
    pitch: f32,
    /// Camera distance multiplier.
    #[arg(long, default_value_t = 1.0)]
    zoom: f32,
    /// Don't wait for vsync (to measure the real frame rate).
    #[arg(long)]
    uncapped: bool,
}

fn main() -> AppExit {
    let args = Args::parse();
    let site = SiteParams {
        context: args.context,
        buildings: args.buildings,
        relief: args.relief,
        building: BuildingParams {
            seed: args.seed,
            floors: args.floors,
            size: Size {
                width: args.width,
                depth: args.depth,
            },
            building_type: args.building_type,
            rooms: args.rooms,
            garden: args.garden,
            style: args.style,
            shape: args.shape,
        },
    };
    let quality = match args.quality {
        QualityArg::Low => Quality::Low,
        QualityArg::High => Quality::High,
    };
    let mut project = Project::new(site, quality);
    let mut tool = Tool::default();
    let mut pointer = Pointer::default();
    match args.demo {
        Some(Demo::Build) => stage_build_demo(&mut project, &mut tool, &mut pointer),
        Some(Demo::Terrain) => stage_terrain_demo(&mut project, &mut tool, &mut pointer),
        None => {}
    }

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Urbanitas · Noj Suon".into(),
            resolution: (1440u32, 900u32).into(),
            present_mode: if args.uncapped {
                PresentMode::AutoNoVsync
            } else {
                PresentMode::AutoVsync
            },
            ..default()
        }),
        ..default()
    }))
    .add_plugins(EguiPlugin::default())
    .add_plugins(PanOrbitCameraPlugin)
    .insert_resource(project)
    .insert_resource(tool)
    .insert_resource(pointer)
    .insert_resource(scene::CameraStart {
        yaw: args.yaw.to_radians(),
        pitch: args.pitch.to_radians(),
        zoom: args.zoom,
    })
    .add_plugins((
        scene::ScenePlugin,
        tools::ToolsPlugin,
        hud::HudPlugin,
        thumbs::ThumbsPlugin,
    ));
    if let Some(path) = args.screenshot {
        app.insert_resource(capture::Capture::new(path, args.settle))
            .add_systems(Last, capture::capture);
    }
    app.run()
}

/// Build mode, walls cut away, a window selected and an olive tree being placed.
fn stage_build_demo(project: &mut Project, tool: &mut Tool, pointer: &mut Pointer) {
    project.mode = Mode::Build;
    project.walls = WallMode::Cutaway;
    project.level = 0;
    project.selected = project
        .layout
        .parts
        .iter()
        .find(|p| {
            p.level == 0
                && matches!(
                    &p.kind,
                    PartKind::Opening(OpeningPart {
                        kind: urbanitas::model::OpeningKind::Window,
                        ..
                    })
                )
        })
        .map(|p| p.id);
    tool.category = Category::Garden;
    tool.pick(Some(Item::Prop(urbanitas::parts::PropKind::OliveTree)));
    let bounds = project.layout.bounds();
    let (cx, _) = bounds.center();
    pointer.forced = Some([cx - 5.0, bounds.z + 2.0]);
    project.status = "Demo: placing an olive tree".into();
    project.mark_all();
}

/// Build mode with the terrain tools out and a hill already raised.
fn stage_terrain_demo(project: &mut Project, tool: &mut Tool, pointer: &mut Pointer) {
    project.mode = Mode::Build;
    let bounds = project.layout.bounds();
    let (cx, cz) = bounds.center();
    let hill = [cx + bounds.w * 0.35, cz - bounds.d * 0.25];
    let area = bounds.grow(40.0);
    project.edit(&[0], None, |layout| {
        let terrain = layout.terrain.get_or_insert_with(|| {
            urbanitas::terrain::Terrain::flat(area, (area.w.max(area.d) / 96.0).max(0.75))
        });
        for _ in 0..12 {
            terrain.brush(urbanitas::terrain::Brush::Raise, hill, 9.0, 0.6);
        }
    });
    project.mark_all();
    tool.category = Category::Terrain;
    tool.pick(Some(Item::Terrain(Sculpt::Raise)));
    tool.brush_radius = 9.0;
    pointer.forced = Some(hill);
    project.status = "Demo: raising a hill".into();
}
