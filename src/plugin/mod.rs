mod database;
pub use database::*;

mod multiplexer;
pub use multiplexer::*;

mod systems;
pub use systems::*;

use bevy::prelude::*;
use crate::prelude::*;

#[derive(Resource, Clone)]
pub struct FluxConfig {
    pub peer_id: Thing,
    pub multiplexer: PeerMultiplexer,
    pub listener: PeerMultiplexerReceiver,
}

impl FluxConfig {
    pub fn new(peer_id: Thing, multiplexer: PeerMultiplexer) -> Self {
        Self {
            peer_id: peer_id.clone(),
            multiplexer: multiplexer.clone(),
            listener: multiplexer.get_receiver(peer_id),
        }
    }
}

pub struct FluxPlugin {
    config: FluxConfig,
}

impl FluxPlugin {
    pub fn new(config: FluxConfig) -> Self {
        Self { config }
    }
}

impl Plugin for FluxPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .add_event::<NetworkEvent>();
    }
}
