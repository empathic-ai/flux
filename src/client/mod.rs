use bevy::prelude::*;
use crate::prelude::*;

pub struct FluxClientPlugin {
    config: FluxConfig
}

impl FluxClientPlugin {
    pub fn new(config: FluxConfig) -> Self {
        Self {
            config
        }
    }
}

impl Plugin for FluxClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((FluxPlugin::new(self.config.clone())));
    }
}
