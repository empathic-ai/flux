mod database;
pub use database::*;

#[cfg(feature = "futures")]
use bevy_async_ecs::{AsyncEcsPlugin, AsyncWorld};
#[cfg(feature = "tokio")]
use bevy_wasm_tasks::*;

mod multiplexer;
pub use multiplexer::*;

mod systems;
pub use systems::*;

use bevy::{prelude::*, reflect::DynamicStruct};
use crate::prelude::*;

use common::prelude::*;
#[cfg(feature = "bevy_std")]
use bevy_simple_subsecond_system::prelude::*;

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
    pub fn send_ev<T>(&self, recipient_id: Id, ev: T) where T: Struct {
        self.channel.send_ev(recipient_id, ev);
    }

    pub fn get_channel_mut(&mut self) -> &mut Channel {
        &mut self.channel
    }

    pub fn clone_channel(&self) -> Channel {
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
    config: FluxConfig,
}

impl FluxPlugin {
    pub fn new(config: FluxConfig) -> Self {
        Self {
            config
        }
    }
}

impl Plugin for FluxPlugin {
    fn build(&self, app: &mut App) {
        app
            .insert_state(DbState::Connecting)
            .insert_resource(self.config.clone())
            .insert_resource(BindingsConfig::default())
            .add_event::<NetworkEvent>()
            .add_event::<PeerEvent>()
            .add_systems(Update, (relay_network_events).run_if(in_state(DbState::Connected)));
        
        #[cfg(feature = "bevy_std")]
        app
            .add_plugins(SimpleSubsecondPlugin::default());

        #[cfg(feature = "surrealdb")]
        app
            .add_systems(PreStartup, (startup, database::start).chain());

        #[cfg(feature = "futures")]
        app
            .add_plugins((AsyncEcsPlugin));

        #[cfg(feature = "tokio")]
        app
            .add_plugins((TasksPlugin::default()));
    }
}

fn startup(world: &mut World) {
    #[cfg(feature = "futures")]
    {
        let async_runner = AsyncRunner::from_world(world);
        world.insert_resource(async_runner);
    }
}

pub fn relay_network_events(
    mut session: ResMut<Session>,
    mut peer_evs: ResMut<Events<PeerEvent>>, mut network_evs: ResMut<Events<NetworkEvent>>,
) {
    for ev in peer_evs.get_cursor().read(&peer_evs) {
        //info!("Sending network event of type {:?}!", ev.network_event.as_ref().unwrap().network_event_type.clone().unwrap());
        session.get_multiplexer().send(ev.peer_id.clone().unwrap(), ev.network_event.clone().unwrap());
    }
    peer_evs.clear();

    //info!("Trying to receive network events...");
    if let Some(ev) = session.get_channel_mut().try_recv() {
        //info!("Relaying network event {}!", ev.get_ev_name());
        network_evs.send(ev);
    }
}
