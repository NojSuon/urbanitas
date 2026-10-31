//! `--screenshot`: render until the scene settles, save a PNG and quit.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::project::Project;

#[derive(Resource)]
pub struct Capture {
    pub path: PathBuf,
    /// How long the scene must stay unchanged first, so shaders compile and textures upload
    /// (slow on integrated GPUs).
    settle: Duration,
    calm_since: Option<Instant>,
    requested_at: Option<Instant>,
}

impl Capture {
    pub fn new(path: PathBuf, settle_secs: f32) -> Self {
        Self {
            path,
            settle: Duration::from_secs_f32(settle_secs),
            calm_since: None,
            requested_at: None,
        }
    }
}

pub fn capture(
    mut commands: Commands,
    mut cap: ResMut<Capture>,
    project: Res<Project>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = Instant::now();
    if let Some(requested) = cap.requested_at {
        let saved = cap.path.metadata().is_ok_and(|m| m.len() > 0);
        if saved || now - requested > Duration::from_secs(30) {
            if !saved {
                error!("screenshot was not written to {}", cap.path.display());
            }
            exit.write(AppExit::Success);
        }
        return;
    }
    if project.is_dirty() || project.regenerate_at.is_some() {
        cap.calm_since = None;
        return;
    }
    let calm = *cap.calm_since.get_or_insert(now);
    if now - calm >= cap.settle {
        let _ = std::fs::remove_file(&cap.path);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(cap.path.clone()));
        cap.requested_at = Some(now);
    }
}
