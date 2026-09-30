use crate::prelude::*;
#[cfg(feature = "client")]
use anyhow::anyhow;
use bevy::prelude::*;

#[derive(Resource, Clone)]
pub struct Session {
    multiplexer: Multiplexer,
    channel: Channel,
    component_requests: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<(Id, String, Entity)>>>,
}

impl Session {
    pub(crate) fn request_record_component(&self, id: Id, component_type: String, entity: Entity) {
        if self.component_requests.lock().unwrap().insert((id, component_type.clone(), entity)) {
            self.send_ev(Id::nil(), TrackRecordComponentEvent { entity_id: id, component_type });
        }
    }

    pub fn new(peer_id: Id) -> Self {
        let multiplexer = Multiplexer::new();
        Self {
            multiplexer: multiplexer.clone(),
            channel: multiplexer.get_channel(peer_id),
            component_requests: Default::default(),
        }
    }

    /// Sends an event using the current user's channel.
    pub fn send_ev<T>(&self, recipient_id: Id, ev: T)
    where
        T: Struct,
    {
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

#[cfg(all(feature = "futures", feature = "tokio"))]
pub(super) fn start(
    config: Res<FluxConfig>,
    runner: Res<AsyncRunner>,
    tasks: bevy_wasm_tasks::Tasks,
    mut state: ResMut<NextState<SessionState>>,
) {
    state.set(SessionState::Establishing);
    let world = runner.get_async_world();
    let api_url = config.get_client_api_url();
    tasks.spawn_auto(async move |_| {
        match get_peer_id(api_url).await {
            Ok(peer_id) => {
                world.apply(move |world: &mut World| {
                    world.insert_resource(Session::new(peer_id));
                    world.resource_mut::<NextState<SessionState>>().set(SessionState::Ready);
                }).await;
            }
            Err(error) => {
                tracing::error!(%error, "Client session initialization failed");
                world.apply(|world: &mut World| {
                    world.resource_mut::<NextState<SessionState>>().set(SessionState::Failed);
                }).await;
            }
        }
    });
}

#[cfg(feature = "client")]
async fn get_peer_id(api_url: String) -> anyhow::Result<Id> {
    let peer_id = match is_session(api_url.clone()).await {
        Ok(client_id) if !client_id.is_empty() => client_id,
        Ok(_) => register(api_url).await?,
        Err(error) => {
            tracing::info!(%error, "Error grabbing session");
            register(api_url).await?
        }
    };
    Id::try_from(&peer_id).map_err(|error| anyhow!("Invalid client ID returned by the API: {error}"))
}

#[cfg(not(feature = "client"))]
async fn get_peer_id(_: String) -> anyhow::Result<Id> {
    Ok(Id::nil())
}

#[cfg(feature = "client")]
pub async fn is_session(api_url: String) -> reqwest::Result<String> {
    let client = reqwest::Client::new();
    let mut request = client.post(format!("{api_url}/session"));
    #[cfg(target_arch = "wasm32")]
    {
        request = request.fetch_credentials_include();
    }
    request.send().await?.error_for_status()?.text().await
}

#[cfg(feature = "client")]
pub async fn register(api_url: String) -> reqwest::Result<String> {
    let client = reqwest::Client::new();
    let mut request = client.post(format!("{api_url}/register"));
    #[cfg(target_arch = "wasm32")]
    {
        request = request.fetch_credentials_include();
    }
    request.send().await?.error_for_status()?.text().await
}