//! Urbanitas, the interactive app: generate a building live from parameters, then edit it
//! part by part in a Sims-style build mode. By Noj Suon.

mod capture;
mod files;
mod hud;
mod project;
mod scene;
mod theme;
mod tools;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PresentMode;
use bevy_egui::EguiPlugin;
use bevy_panorbit_camera::PanOrbitCameraPlugin;
use clap::{Parser, ValueEnum};
use urbanitas::mesh::build::WallMode;
use urbanitas::parts::{OpeningPart, PartKind, PropKind, PropPart};
use urbanitas::{ArchitectureStyle, BuildingParams, BuildingType, Garden, Size};

use project::{Mode, Project, Quality};
use tools::{Item, Pointer, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum QualityArg {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Demo {
    /// Start in build mode with a few hand-placed parts, the cutaway view and a placement ghost.
    Build,
}

/// Urbanitas: procedural architecture you can edit.
#[derive(Parser, Debug)]
#[command(name = "urbanitas-app", version, author = "Noj Suon")]
struct Args {
    #[arg(long, value_enum, default_value_t = ArchitectureStyle::Florentine)]
    style: ArchitectureStyle,
    #[arg(long = "type", value_enum, default_value_t = BuildingType::Villa)]
    building_type: BuildingType,
    #[arg(long, default_value_t = 2)]
    floors: u32,
    #[arg(long, default_value_t = 16.0)]
    width: f64,
    #[arg(long, default_value_t = 12.0)]
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
    /// Don't wait for vsync (to measure the real frame rate).
    #[arg(long)]
    uncapped: bool,
    /// Camera pitch in degrees.
    #[arg(long, default_value_t = 26.0)]
    pitch: f32,
    /// Camera distance multiplier.
    #[arg(long, default_value_t = 1.0)]
    zoom: f32,
}

fn main() -> AppExit {
    let args = Args::parse();
    let params = BuildingParams {
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
    };
    let quality = match args.quality {
        QualityArg::Low => Quality::Low,
        QualityArg::High => Quality::High,
    };
    let mut project = Project::new(params, quality);
    let mut tool = Tool::default();
    let mut pointer = Pointer::default();
    if args.demo == Some(Demo::Build) {
        stage_build_demo(&mut project, &mut tool, &mut pointer);
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
    .add_plugins((scene::ScenePlugin, tools::ToolsPlugin, hud::HudPlugin));
    if let Some(path) = args.screenshot {
        app.insert_resource(capture::Capture::new(path, args.settle))
            .add_systems(Last, capture::capture);
    }
    app.run()
}

/// Build mode, walls cut away, a few hand-placed parts and a ghost under a pinned pointer.
fn stage_build_demo(project: &mut Project, tool: &mut Tool, pointer: &mut Pointer) {
    project.mode = Mode::Build;
    project.walls = WallMode::Cutaway;
    project.level = 0;
    let bounds = project.layout.bounds();
    let (cx, cz) = bounds.center();
    let fountain = [cx + 4.0, bounds.z + 3.5];
    project.edit(&[0], None, |layout| {
        layout.add(
            0,
            PartKind::Prop(PropPart {
                kind: PropKind::Fountain,
                at: fountain,
                yaw: 0.0,
                scale: 1.0,
                height: 1.5,
            }),
        );
        for dx in [-2.0, 2.0] {
            layout.add(
                0,
                PartKind::Prop(PropPart {
                    kind: PropKind::Lamp,
                    at: [cx + dx, bounds.z + 1.0],
                    yaw: 0.0,
                    scale: 1.0,
                    height: 2.6,
                }),
            );
        }
    });
    project.selected = project
        .layout
        .parts
        .iter()
        .find(|p| {
            matches!(
                &p.kind,
                PartKind::Opening(OpeningPart {
                    kind: urbanitas::model::OpeningKind::Window,
                    ..
                })
            ) && p.level == 0
        })
        .map(|p| p.id);
    tool.pick(Some(Item::Prop(PropKind::OliveTree)));
    pointer.forced = Some([cx - 5.0, cz - 9.0]);
    project.status = "Demo: placing an olive tree".into();
    project.mark_all();
}
