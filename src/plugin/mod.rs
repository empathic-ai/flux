mod database;
use bevy_async_ecs::AsyncWorld;
pub use database::*;

mod multiplexer;
pub use multiplexer::*;

mod systems;
pub use systems::*;

use bevy::{prelude::*, reflect::DynamicStruct, utils::error};
use bevy_wasm_tasks::*;
use crate::prelude::*;

#[derive(Resource, Clone)]
pub struct Session {
    multiplexer: Multiplexer,
    channel: Channel,
}

impl Session {
    pub fn new(peer_id: Id) -> Self {
        let multiplexer = Multiplexer::new();
        Self {
            multiplexer: multiplexer.clone(),
            channel: multiplexer.get_channel(peer_id),
        }
    }

    ///  Sends an event using the current user's channel.
    pub fn send_ev<T>(&self, peer_id: Id, ev: T) where T: Struct {
        self.channel.send_ev(peer_id, ev);
    }

    pub fn get_channel(&self) -> Channel {
        self.channel.clone()
    }

    pub fn get_peer_channel(&self, id: Id) -> Channel {
        self.multiplexer.get_channel(id)
    }

    pub fn get_multiplexer(&self) -> Multiplexer {
        self.multiplexer.clone()
    }

    pub fn get_id(&self) -> Id {
        self.channel.get_id()
    }
}

pub struct FluxPlugin {
    config: Session,
}

impl FluxPlugin {
    pub fn new(config: Session) -> Self {
        Self { config }
    }
}

impl Plugin for FluxPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_state(DbState::Connecting)
            .insert_resource(self.config.clone())
            .add_event::<NetworkEvent>()
            .add_event::<PeerEvent>()
            .add_systems(PreStartup, (startup, database::start.map(error)).chain())
            .add_plugins(TasksPlugin::default());
    }
}

fn startup(world: &mut World) {
    let async_tasks = AsyncRunner::from_world(world);
    world.insert_resource(async_tasks);
}
