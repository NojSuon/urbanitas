//! `urbanitas` command-line tool.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use urbanitas::export::{json, obj, svg};
use urbanitas::{
    ArchitectureStyle, Building, BuildingParams, BuildingType, Context, Garden, LandscapeParams,
    Relief, Shape, SiteParams, Size, TownParams, WaterBody, generate, generate_site, grow_town,
    landscape_layout, suggest_piazza,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    /// 3D mesh (.obj) with its material library (.mtl).
    Obj,
    /// One floor plan per floor (.svg).
    Svg,
    /// The building as an app project file (.urbanitas.json).
    Json,
}

/// Procedurally generate buildings and export them as OBJ meshes, SVG floor plans and JSON.
#[derive(Parser, Debug)]
#[command(name = "urbanitas", version, author = "Noj Suon")]
struct Cli {
    /// Number of floors.
    #[arg(long, default_value_t = 2)]
    floors: u32,
    /// Footprint width (west → east), in metres.
    #[arg(long, default_value_t = 12.0)]
    width: f64,
    /// Footprint depth (front → back), in metres.
    #[arg(long, default_value_t = 10.0)]
    depth: f64,
    /// What the building is used for.
    #[arg(long = "type", value_enum, default_value_t = BuildingType::House)]
    building_type: BuildingType,
    /// Rooms per floor, not counting the hall and stairs.
    #[arg(long, default_value_t = 4)]
    rooms: u32,
    /// Where the garden goes.
    #[arg(long, value_enum, default_value_t = Garden::Front)]
    garden: Garden,
    /// Architectural style.
    #[arg(long, value_enum, default_value_t = ArchitectureStyle::Mediterranean)]
    style: ArchitectureStyle,
    /// Plan shape of the footprint.
    #[arg(long, value_enum, default_value_t = Shape::Auto)]
    shape: Shape,
    /// The setting: one building, or a whole urban, suburban or rural site.
    #[arg(long, value_enum, default_value_t = Context::Single)]
    context: Context,
    /// How many buildings an urban or suburban site gets.
    #[arg(long, default_value_t = 6)]
    buildings: u32,
    /// Height of the hills, in metres (0 for flat ground).
    #[arg(long, default_value_t = 0.0)]
    relief: f64,
    /// Generate a whole landscape with this relief and grow a town on it (instead of --context).
    #[arg(long, value_enum)]
    landscape: Option<Relief>,
    /// The landscape's body of water.
    #[arg(long, value_enum, default_value_t = WaterBody::Beach)]
    water: WaterBody,
    /// Side of the landscape's square map, in metres.
    #[arg(long, default_value_t = 320.0)]
    map_size: f64,
    /// How many houses the town on a landscape gets (0 for bare land).
    #[arg(long, default_value_t = 24)]
    town: u32,
    /// Random seed; the same seed and parameters always give the same building. Random if omitted.
    #[arg(long)]
    seed: Option<u64>,
    /// Read all building parameters from a JSON file instead of the flags above.
    #[arg(long, value_name = "FILE")]
    params: Option<PathBuf>,
    /// Output formats, comma separated.
    #[arg(long, value_enum, value_delimiter = ',', default_values_t = [Format::Obj, Format::Svg, Format::Json])]
    format: Vec<Format>,
    /// Output directory.
    #[arg(long, short, default_value = "output")]
    out: PathBuf,
    /// Base file name for the outputs. Defaults to urbanitas_<type>_<seed>.
    #[arg(long)]
    name: Option<String>,
    /// Size of the OBJ's PNG textures, in pixels (a power of two).
    #[arg(long, default_value_t = 512, value_parser = texture_size)]
    texture_size: u32,
}

fn texture_size(text: &str) -> Result<u32, String> {
    let size: u32 = text.parse().map_err(|e| format!("{e}"))?;
    if size.is_power_of_two() && (16..=2048).contains(&size) {
        Ok(size)
    } else {
        Err("must be a power of two between 16 and 2048".into())
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let params = match &cli.params {
        Some(path) => serde_json::from_str::<BuildingParams>(&fs::read_to_string(path)?)?,
        None => BuildingParams {
            seed: cli.seed.unwrap_or_else(random_seed),
            floors: cli.floors,
            size: Size {
                width: cli.width,
                depth: cli.depth,
            },
            building_type: cli.building_type,
            rooms: cli.rooms,
            garden: cli.garden,
            style: cli.style,
            shape: cli.shape,
        },
    };
    let building = generate(&params)?;
    let site = SiteParams {
        context: cli.context,
        building: params.clone(),
        buildings: cli.buildings,
        relief: cli.relief,
    };
    let stem = cli
        .name
        .as_deref()
        .map(sanitize)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match (cli.landscape, cli.context) {
            (Some(relief), _) => {
                format!("urbanitas_town_{relief:?}_{:?}_{}", cli.water, params.seed).to_lowercase()
            }
            (None, Context::Single) => building.name.clone(),
            (None, context) => format!("urbanitas_{}_{}", site_slug(context), params.seed),
        });

    let layout = match cli.landscape {
        Some(relief) => {
            let land = LandscapeParams {
                relief,
                water: cli.water,
                size: cli.map_size,
                seed: params.seed,
            };
            let mut layout = landscape_layout(&land, params.style);
            if cli.town > 0 {
                let plaza = suggest_piazza(&layout).ok_or("no dry ground for a town")?;
                let town = TownParams {
                    style: params.style,
                    houses: cli.town,
                    seed: params.seed,
                    ..TownParams::default()
                };
                let report = grow_town(&mut layout, plaza, &town, &[])?;
                println!(
                    "Town: {} houses on {} streets",
                    report.houses, report.streets
                );
            }
            layout
        }
        None => generate_site(&site)?,
    };

    fs::create_dir_all(&cli.out)?;
    let mut written = Vec::new();
    if cli.format.contains(&Format::Obj) {
        let path = cli.out.join(format!("{stem}.obj"));
        written.extend(obj::write_obj(&layout, &path, cli.texture_size)?);
    }
    if cli.format.contains(&Format::Svg) {
        for level in 0..layout.levels() {
            let path = cli.out.join(format!("{stem}_floor{level}.svg"));
            fs::write(&path, svg::floor_plan_svg(&layout, level))?;
            written.push(path);
        }
    }
    if cli.format.contains(&Format::Json) {
        let path = cli.out.join(format!("{stem}.urbanitas.json"));
        fs::write(&path, json::layout_to_json(&layout))?;
        written.push(path);
    }

    if cli.landscape.is_none() && cli.context == Context::Single {
        print_summary(&building);
    } else {
        println!("Urbanitas · {}", layout.description);
    }
    written.sort();
    for path in written {
        println!("  wrote {}", path.display());
    }
    Ok(())
}

fn print_summary(b: &Building) {
    println!(
        "Urbanitas · {} {} · {} plan · seed {} · {:.1} × {:.1} m · {} floor(s) · {:.1} m tall",
        b.params.style.label(),
        b.params.building_type.label(),
        b.shape.label(),
        b.params.seed,
        b.footprint.w,
        b.footprint.d,
        b.floors.len(),
        b.height()
    );
    println!(
        "  roof: {} · palette: {} · garden: {}",
        b.roof_label(),
        b.palette.name,
        b.params.garden.label()
    );
    for floor in &b.floors {
        let rooms: Vec<String> = floor
            .rooms
            .iter()
            .map(|r| format!("{} {:.1} m²", r.kind.label(), r.area))
            .collect();
        println!("  {}: {}", floor.name(), rooms.join(", "));
    }
}

fn site_slug(context: Context) -> &'static str {
    match context {
        Context::Single => "building",
        Context::Urban => "urban",
        Context::Suburban => "suburban",
        Context::Rural => "rural",
    }
}

fn random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// Keeps file names to letters, digits, `-` and `_`.
fn sanitize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}
