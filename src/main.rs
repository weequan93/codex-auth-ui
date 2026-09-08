#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(not(target_os = "macos"))]
use codex_account_hub::app::PARKED_POSITION;
use codex_account_hub::{
    app::{AccountHubApp, WINDOW_SIZE},
    icon,
};
use eframe::egui;

#[cfg(target_os = "macos")]
extern "C" {
    fn cah_hide_dock_icon();
    fn cah_print_bundle_diagnostics() -> bool;
}

fn main() -> eframe::Result {
    #[cfg(target_os = "macos")]
    if std::env::args_os().any(|arg| arg == "--diagnose-startup") {
        // Runs before AppKit UI or the account worker starts. Packaging smoke test only.
        std::process::exit(if unsafe { cah_print_bundle_diagnostics() } {
            0
        } else {
            1
        });
    }
    #[cfg(target_os = "macos")]
    unsafe {
        cah_hide_dock_icon();
    }

    let viewport_icon = egui::IconData {
        rgba: icon::rgba_icon(64),
        width: 64,
        height: 64,
    };
    #[cfg(not(target_os = "macos"))]
    let start_visible = codex_account_hub::startup::start_visible();
    let viewport = egui::ViewportBuilder::default()
        .with_title("Codex Account Hub")
        .with_inner_size(WINDOW_SIZE)
        .with_min_inner_size(WINDOW_SIZE)
        .with_max_inner_size(WINDOW_SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_taskbar(false)
        .with_visible(true)
        .with_icon(viewport_icon);
    #[cfg(not(target_os = "macos"))]
    let viewport = if start_visible {
        viewport
    } else {
        viewport.with_position(PARKED_POSITION)
    };
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "codex-account-hub",
        options,
        Box::new(|creation| {
            #[cfg(feature = "visual-qa")]
            if let Ok(scenario) = std::env::var("CODEX_ACCOUNT_HUB_PREVIEW") {
                return Ok(Box::new(AccountHubApp::preview(
                    &creation.egui_ctx,
                    &scenario,
                )));
            }
            Ok(Box::new(AccountHubApp::new(&creation.egui_ctx)))
        }),
    )
}
