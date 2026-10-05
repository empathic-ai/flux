mod database;
use bevy_trait_query::RegisterExt;
pub use database::*;

mod session;
pub use session::Session;
#[cfg(feature = "client")]
pub use session::{is_session, register};

mod commands;
pub use commands::*;

#[cfg(all(feature = "bevy_std", feature = "futures"))]
mod executor;
#[cfg(all(feature = "bevy_std", feature = "futures"))]
pub use executor::*;

#[cfg(all(feature = "bevy_std", feature = "futures"))]
mod service;
#[cfg(all(feature = "bevy_std", feature = "futures"))]
pub use service::*;

#[cfg(all(feature = "bevy_std", feature = "futures"))]
mod rpc;
#[cfg(all(feature = "bevy_std", feature = "futures"))]
pub use rpc::*;

#[cfg(feature = "futures")]
use bevy_async_ecs::{AsyncEcsPlugin, AsyncWorld};
#[cfg(feature = "tokio")]
use bevy_wasm_tasks::*;

pub use crate::multiplexer::*;

mod systems;
pub use systems::*;

use crate::prelude::*;
use bevy::{ecs::system::SystemState, prelude::*, reflect::DynamicStruct};

#[cfg(feature = "subsecond")]
use bevy_simple_subsecond_system::prelude::*;
use common::prelude::*;

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
        #[cfg(feature = "bevy_std")]
        if !app.is_plugin_added::<LazyViewPlugin>() {
            app.add_plugins(LazyViewPlugin);
        }

        if !app.is_plugin_added::<BindingGraphPlugin>() {
            app.add_plugins(BindingGraphPlugin);
        }

        app.init_resource::<EditBindings>()
            .register_component_as::<dyn Reactive, EditStatus>()
            .register_component_as::<dyn Reactive, EditInputStatus>()
            .add_systems(Update, update_edit_bindings.in_set(EditBindingSet));

        app.init_resource::<ChangeBindings>()
            .add_systems(Update, update_change_bindings);

        app.add_reactive::<ReactiveMapView>()
            .add_reactive::<ReactiveMapKey>();

        app.init_state::<DatabaseState>()
            .init_state::<SessionState>()
            .insert_resource(self.config.clone())
            .add_event::<NetworkEvent>()
            .add_event::<PeerEvent>()
            .add_systems(
                Update,
                relay_network_events.run_if(in_state(SessionState::Ready)),
            )
            .add_systems(
                PostUpdate,
                (process_reactive_lists, process_reactive_maps)
                    .after(BindingGraphSet)
                    .run_if(in_state(DatabaseState::Ready)),
            );

        #[cfg(feature = "subsecond")]
        app.add_plugins(SimpleSubsecondPlugin::default());

        #[cfg(all(feature = "futures", feature = "tokio"))]
        app.add_systems(PreStartup, (startup, session::start).chain());

        #[cfg(feature = "surrealdb")]
        app.add_systems(OnEnter(SessionState::Ready), database::start)
            .add_systems(OnEnter(DatabaseState::Connected), database::prepare_database);

        #[cfg(feature = "futures")]
        app.add_plugins((AsyncEcsPlugin));

        #[cfg(feature = "tokio")]
        app.add_plugins((TasksPlugin::default()));
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
    mut peer_evs: ResMut<Events<PeerEvent>>,
    mut network_evs: ResMut<Events<NetworkEvent>>,
) {
    for ev in peer_evs.get_cursor().read(&peer_evs) {
        //info!("Sending network event of type {:?}!", ev.network_event.as_ref().unwrap().network_event_type.clone().unwrap());
        session.get_multiplexer().send(
            ev.peer_id.clone().unwrap(),
            ev.network_event.clone().unwrap(),
        );
    }
    peer_evs.clear();

    //info!("Trying to receive network events...");
    while let Some(ev) = session.get_channel_mut().try_recv() {
        //info!("Relaying network event {}!", ev.get_ev_name());
        network_evs.send(ev);
    }
}

pub trait NetworkCommandsExt {
    fn send_network_event<T>(&mut self, recipient_id: Id, ev: T)
    where
        T: Struct;
}

// implement our trait for Bevy's `Commands`
impl<'w, 's> NetworkCommandsExt for Commands<'w, 's> {
    fn send_network_event<T>(&mut self, recipient_id: Id, ev: T)
    where
        T: Struct,
    {
        self.queue(move |world: &mut World| {
            let mut system_state: SystemState<(ResMut<Session>)> = SystemState::new(world);
            {
                let (mut session) = system_state.get_mut(world);
                session.send_ev(recipient_id, ev);
            }
            system_state.apply(world);
        });
    }
}
