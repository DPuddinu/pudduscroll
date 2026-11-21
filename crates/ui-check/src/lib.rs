mod align;
mod gap;
mod image;
mod raster;
mod scene;

pub use align::ListAlign;
pub use gap::ElementGap;
pub use image::Image;
pub use raster::Raster;
pub use scene::{PxRect, Scene};

pub trait Check {
    fn name(&self) -> &'static str;
    fn run(&self, scene: &Scene<'_>) -> Vec<Finding>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub check: &'static str,
    pub message: String,
    pub bounds: PxRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub items: usize,
}

impl Report {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Default)]
pub struct Suite {
    checks: Vec<Box<dyn Check>>,
}

impl Suite {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, check: impl Check + 'static) -> Self {
        self.checks.push(Box::new(check));
        self
    }

    #[must_use]
    pub fn run(&self, image: &Image) -> Report {
        let scene = Scene::read(image);
        let mut findings = Vec::new();
        for check in &self.checks {
            findings.extend(check.run(&scene));
        }
        Report {
            findings,
            items: scene.items.len(),
        }
    }
}

impl std::fmt::Debug for Suite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<_> = self.checks.iter().map(|check| check.name()).collect();
        f.debug_struct("Suite").field("checks", &names).finish()
    }
}

#[must_use]
pub fn standard() -> Suite {
    Suite::new()
        .with(ListAlign::default())
        .with(ElementGap::default())
}

#[cfg(test)]
mod tests;
