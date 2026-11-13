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
