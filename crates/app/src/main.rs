//! eleven: an XMMS-style audio player. It goes to eleven.
//!
//! Usage: `eleven [FILE | FOLDER | PLAYLIST | URL]...` — arguments are added to the playlist and
//! the first one starts playing.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use eframe::egui;
use eleven_app::backend::{EngineBackend, NativeFileChooser};
use eleven_app::paths::AppPaths;
use eleven_app::ui::layout::{Frame, Sections};
use eleven_app::{Controller, ElevenApp, Persistence};
use eleven_audio::{MetadataScanner, Notify};
use eleven_core::Config;

/// Adds command-line arguments to the playlist and plays the first one.
fn enqueue_args(controller: &mut Controller, args: &[String]) {
    let before = controller.playlist().len();
    for arg in args {
        let lower = arg.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            controller.add_location(arg);
        } else {
            controller.add_paths(&[PathBuf::from(arg)]);
        }
    }
    if controller.playlist().len() > before {
        controller.play_index(before);
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let paths = AppPaths::detect();
    let config = paths.as_ref().map_or_else(Config::default, |p| {
        Config::load(&p.config_file()).unwrap_or_else(|e| {
            log::warn!("{e}; using defaults");
            Config::default()
        })
    });
    let persistence = paths
        .as_ref()
        .map_or_else(Persistence::default, AppPaths::persistence);
    let skins_dir = paths.as_ref().map(AppPaths::skins_dir);
    if let Some(dir) = &skins_dir {
        let _ = std::fs::create_dir_all(dir);
    }

    // Background threads wake the UI through this once the egui context exists.
    let ctx_slot: Arc<OnceLock<egui::Context>> = Arc::new(OnceLock::new());
    let slot = Arc::clone(&ctx_slot);
    let notify: Notify = Arc::new(move || {
        if let Some(ctx) = slot.get() {
            ctx.request_repaint();
        }
    });

    let backend = EngineBackend::start(config.output_device.clone(), Arc::clone(&notify));
    let tags = MetadataScanner::new(2, &notify);
    let media_notify = Arc::clone(&notify);
    let media = eleven_app::media_keys::create(move || media_notify());
    let sections = Sections {
        shaded: config.shaded,
        equalizer: config.show_equalizer,
        playlist: config.show_playlist,
        rows: config.playlist_rows,
    };
    let zoom = if config.double_size { 2.0 } else { 1.0 };
    let size = Frame::compute(egui::Pos2::ZERO, sections).size() * zoom;
    let decorations = config.decorations;

    let mut controller = Controller::new(
        config,
        Box::new(backend),
        Box::new(tags),
        media,
        persistence,
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    enqueue_args(&mut controller, &args);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("eleven")
        .with_app_id("eleven")
        .with_inner_size(size)
        .with_resizable(false)
        .with_decorations(decorations);
    match eframe::icon_data::from_png_bytes(include_bytes!("../../../packaging/icon-256.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(e) => log::warn!("icon: {e}"),
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "eleven",
        options,
        Box::new(move |cc| {
            let _ = ctx_slot.set(cc.egui_ctx.clone());
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ElevenApp::new(
                controller,
                Box::new(NativeFileChooser),
                skins_dir,
            )))
        }),
    )
}
