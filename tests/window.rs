//! Draws the real window with fake accounts (src/system_demo.rs) and saves target/window-*.png, so the layout
//! can be checked without Windows: an account page at the default size and maximized on a 1080p screen, and
//! the Whole PC page. Needs a GPU or a software Vulkan driver (lavapipe): cargo test --test window -- --ignored
#![cfg(not(windows))]

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn harness(size: [f32; 2]) -> Harness<'static, potato::app::App> {
    let mut h = Harness::builder().with_size(size).build_eframe(|cc| potato::app::App::new(cc));
    h.ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    h.run_steps(6);
    h
}

fn save(h: &mut Harness<'static, potato::app::App>, name: &str) {
    let image = h.render().expect("render");
    image.save(format!("{}/target/window-{name}.png", env!("CARGO_MANIFEST_DIR"))).expect("save");
}

#[test]
#[ignore]
fn draw_the_window() {
    save(&mut harness([1000.0, 760.0]), "default");
    save(&mut harness([1920.0, 1000.0]), "maximized");
    let mut h = harness([1000.0, 760.0]);
    h.get_by_label("Whole PC").click();
    h.run_steps(4);
    save(&mut h, "whole-pc");
}
