//mod axum;
//pub use axum::*;

use crate::prelude::*;
use bevy::prelude::*;

pub struct FluxServerPlugin {
    config: FluxConfig,
}

impl FluxServerPlugin {
    pub fn new(config: FluxConfig) -> Self {
        Self { config }
    }
}

impl Plugin for FluxServerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FluxPlugin::new(self.config.clone())));
    }
}
