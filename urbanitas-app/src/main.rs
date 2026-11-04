//! Urbanitas, the interactive app: generate a landscape, grow a town round a piazza (or pick
//! a site preset), then edit it house by house and part by part in a Sims-style build mode.
//! By Noj Suon.

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
    ArchitectureStyle, BuildingParams, BuildingType, Context, DesignMix, Garden, LandscapeParams,
    Relief, Shape, SiteParams, Size, TownParams, WaterBody, suggest_piazza,
};

use hud::{HudState, Panel};
use project::{Project, Quality, Source};
use tools::{Category, Item, Pointer, Sculpt, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum QualityArg {
    Low,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Demo {
    /// The Build panel with the cutaway view, a window selected and an olive tree being placed.
    Build,
    /// The Land options with a brush out and a hill raised next to the buildings.
    Terrain,
    /// The town panel, with the ghost piazza under the pointer.
    Piazza,
    /// A house selected, its card floating beside it.
    House,
    /// The design library, with the selected house saved (in memory only) and a copy
    /// about to be placed.
    Designs,
}

/// Urbanitas: procedural architecture you can edit.
#[derive(Parser, Debug)]
#[command(name = "urbanitas-app", version, author = "Noj Suon")]
struct Args {
    /// Start from a site preset instead of a landscape.
    #[arg(long, value_enum)]
    context: Option<Context>,
    /// The landscape's relief.
    #[arg(long, value_enum, default_value_t = Relief::Hills)]
    landscape: Relief,
    /// The landscape's body of water.
    #[arg(long, value_enum, default_value_t = WaterBody::Beach)]
    water: WaterBody,
    /// Side of the landscape's map, in metres.
    #[arg(long, default_value_t = 320.0)]
    map_size: f64,
    /// Houses in the town grown on the landscape at start (0 for bare land).
    #[arg(long, default_value_t = 24)]
    town: u32,
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
        context: args.context.unwrap_or(Context::Single),
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
    let landscape = LandscapeParams {
        relief: args.landscape,
        water: args.water,
        size: args.map_size,
        seed: args.seed,
    };
    let town = TownParams {
        style: args.style,
        houses: args.town.max(4),
        density: 0.6,
        seed: args.seed,
        mix: DesignMix::Generated,
    };
    let source = if args.context.is_some() {
        Source::Site
    } else {
        Source::Landscape
    };
    let mut project = Project::new(source, site, landscape, town, quality);
    let mut hud_state = HudState::default();
    if source == Source::Landscape && args.town > 0 {
        if let Some(plaza) = suggest_piazza(&project.layout) {
            project.grow_town_at(plaza);
        }
        hud_state.panel = Some(Panel::Town);
    }
    let mut tool = Tool::default();
    let mut pointer = Pointer::default();
    match args.demo {
        Some(Demo::Build) => stage_build_demo(&mut project, &mut tool, &mut pointer),
        Some(Demo::Terrain) => stage_terrain_demo(&mut project, &mut tool, &mut pointer),
        Some(Demo::Piazza) => stage_piazza_demo(&mut project, &mut pointer),
        Some(Demo::House) => stage_house_demo(&mut project),
        Some(Demo::Designs) => {
            stage_house_demo(&mut project);
            stage_designs_demo(&mut project, &mut tool, &mut pointer);
        }
        None => {}
    }
    hud_state.panel = match args.demo {
        Some(Demo::Build) => Some(Panel::Build),
        Some(Demo::Terrain) => Some(Panel::Land),
        Some(Demo::House) => None,
        Some(Demo::Piazza) => Some(Panel::Town),
        Some(Demo::Designs) => Some(Panel::Designs),
        None => hud_state.panel,
    };

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
    .insert_resource(hud_state)
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

/// Walls cut away, a window selected and an olive tree being placed.
fn stage_build_demo(project: &mut Project, tool: &mut Tool, pointer: &mut Pointer) {
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

/// A brush out and a hill already raised.
fn stage_terrain_demo(project: &mut Project, tool: &mut Tool, pointer: &mut Pointer) {
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
    tool.pick(Some(Item::Terrain(Sculpt::Raise)));
    tool.brush_radius = 9.0;
    pointer.forced = Some(hill);
    project.status = "Demo: raising a hill".into();
}

/// The town panel open and the ghost piazza under the pointer, beside the town.
fn stage_piazza_demo(project: &mut Project, pointer: &mut Pointer) {
    project.placing_piazza = true;
    let at = project.plaza.unwrap_or([0.0, 0.0]);
    pointer.forced = Some([at[0] + 70.0, at[1] + 30.0]);
    project.status = "Demo: placing a piazza".into();
}

/// The house nearest the piazza selected.
fn stage_house_demo(project: &mut Project) {
    let plaza = project.plaza.unwrap_or([0.0, 0.0]);
    let near = (0..project.layout.palettes.len() as u32)
        .filter_map(|g| {
            project
                .layout
                .module(g)
                .and_then(|m| m.lot)
                .map(|lot| (g, lot))
        })
        .min_by(|a, b| {
            let d = |r: &urbanitas::geometry::Rect| {
                let (x, z) = r.center();
                (x - plaza[0]).hypot(z - plaza[1])
            };
            d(&a.1).total_cmp(&d(&b.1))
        });
    project.selected_group = near.map(|(g, _)| g);
    project.status = "Demo: a house selected".into();
    project.mark_all();
}

/// Saves the selected house as a design (in memory, not to disk), then arms a copy of it.
fn stage_designs_demo(project: &mut Project, tool: &mut Tool, pointer: &mut Pointer) {
    let Some(group) = project.selected_group else {
        return;
    };
    if let Some(design) =
        urbanitas::design::Design::capture(&project.layout, group, "demo-palazzo", "Palazzo demo")
    {
        project.layout.module_mut(group).design = Some(design.id.clone());
        project.designs.insert(0, design);
    }
    tool.pick(Some(Item::Design(0)));
    tool.yaw = -std::f64::consts::FRAC_PI_2;
    let plaza = project.plaza.unwrap_or([0.0, 0.0]);
    pointer.forced = Some([plaza[0] - 60.0, plaza[1] + 10.0]);
    project.status = "Demo: placing a copy of a saved design".into();
}
