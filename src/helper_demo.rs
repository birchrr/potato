//! Open Maximized off Windows: nothing to do.

#[derive(Default)]
pub struct Maximizer {
    _nothing: (),
}

impl Maximizer {
    pub fn tick(&mut self) {}
}

pub fn run() {}

pub fn install() -> bool {
    true
}

pub fn uninstall() {}

pub fn rejoin_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("potato-demo-rejoin")
}

pub fn serve_rejoins(_: &str, _: &mut u64) {}

pub fn running_here() -> bool {
    true
}
