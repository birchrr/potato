//! Draws the real window with fake accounts (src/system_demo.rs) at its default size and maximized on a
//! 1080p screen, saving target/window-*.png, so the layout can be checked without Windows. Needs a GPU or a
//! software Vulkan driver (lavapipe): cargo test --test window -- --ignored
#![cfg(not(windows))]

use eframe::egui;
use egui_kittest::Harness;

#[test]
#[ignore]
fn draw_the_window() {
    for (name, size) in [("default", [1000.0, 800.0]), ("maximized", [1920.0, 1000.0])] {
        let mut h = Harness::builder().with_size(size).build_eframe(|cc| potato::app::App::new(&cc.egui_ctx));
        h.ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
        h.run_steps(6);
        let image = h.render().expect("render");
        image.save(format!("{}/target/window-{name}.png", env!("CARGO_MANIFEST_DIR"))).expect("save");
    }
}
