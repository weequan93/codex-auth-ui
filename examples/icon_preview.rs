//! Render the shared app and menu-bar artwork without accessing account data.
//! cargo run --example icon_preview --features visual-qa
use codex_account_hub::icon;
use eframe::egui::{self, Color32, ColorImage, RichText, Vec2};

struct Preview {
    app: egui::TextureHandle,
    tray: egui::TextureHandle,
}

impl eframe::App for Preview {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(246, 247, 251)))
            .show(ctx, |ui| {
                ui.add_space(22.0);
                ui.vertical_centered(|ui| {
                    ui.image((self.app.id(), Vec2::splat(144.0)));
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("Account Hub")
                            .size(20.0)
                            .color(Color32::from_rgb(27, 31, 42)),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.add_space(118.0);
                        ui.label(RichText::new("Menu bar").color(Color32::from_rgb(105, 113, 132)));
                        ui.add(
                            egui::Image::new((self.tray.id(), Vec2::splat(22.0)))
                                .tint(Color32::from_rgb(27, 31, 42)),
                        );
                        egui::Frame::new()
                            .fill(Color32::from_rgb(35, 38, 47))
                            .inner_margin(5.0)
                            .corner_radius(5.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Image::new((self.tray.id(), Vec2::splat(22.0)))
                                        .tint(Color32::WHITE),
                                );
                            });
                    });
                });
            });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Account Hub icon preview",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([400.0, 280.0])
                .with_resizable(false),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        },
        Box::new(|cc| {
            let app = cc.egui_ctx.load_texture(
                "app",
                ColorImage::from_rgba_unmultiplied([256, 256], &icon::rgba_icon(256)),
                egui::TextureOptions::LINEAR,
            );
            let mut white_tray = icon::rgba_tray_icon(64);
            // White RGB allows the preview to tint the same alpha mask for either theme.
            for pixel in white_tray.chunks_exact_mut(4) {
                pixel[..3].fill(255);
            }
            let tray = cc.egui_ctx.load_texture(
                "tray",
                ColorImage::from_rgba_unmultiplied([64, 64], &white_tray),
                egui::TextureOptions::LINEAR,
            );
            Ok(Box::new(Preview { app, tray }))
        }),
    )
}
