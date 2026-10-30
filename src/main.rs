//! `urbanitas` command-line tool.

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, ValueEnum};
use urbanitas::export::{json, obj, svg};
use urbanitas::{
    ArchitectureStyle, Building, BuildingParams, BuildingType, Garden, Size, generate,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Format {
    /// 3D mesh (.obj) with its material library (.mtl).
    Obj,
    /// One floor plan per floor (.svg).
    Svg,
    /// The full building model (.json).
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
        },
    };
    let building = generate(&params)?;
    let stem = cli
        .name
        .as_deref()
        .map(sanitize)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| building.name.clone());

    fs::create_dir_all(&cli.out)?;
    let mut written = Vec::new();
    if cli.format.contains(&Format::Obj) {
        let path = cli.out.join(format!("{stem}.obj"));
        obj::write_obj(&building, &path)?;
        written.push(path.with_extension("mtl"));
        written.push(path);
    }
    if cli.format.contains(&Format::Svg) {
        for floor in &building.floors {
            let path = cli.out.join(format!("{stem}_floor{}.svg", floor.level));
            fs::write(&path, svg::floor_plan_svg(&building, floor))?;
            written.push(path);
        }
    }
    if cli.format.contains(&Format::Json) {
        let path = cli.out.join(format!("{stem}.json"));
        fs::write(&path, json::to_json(&building))?;
        written.push(path);
    }

    print_summary(&building);
    written.sort();
    for path in written {
        println!("  wrote {}", path.display());
    }
    Ok(())
}

fn print_summary(b: &Building) {
    println!(
        "Urbanitas · {} {} · seed {} · {:.1} × {:.1} m · {} floor(s) · {:.1} m tall",
        b.params.style.label(),
        b.params.building_type.label(),
        b.params.seed,
        b.footprint.w,
        b.footprint.d,
        b.floors.len(),
        b.height()
    );
    println!(
        "  roof: {} · palette: {} · garden: {}",
        b.roof.label(),
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
