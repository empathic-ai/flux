use anyhow::{Result, ensure, Context};
use flux::prelude::*;
use crate::schema::{Catalog, GraphValue, Schema, ValueMode, MAX_GRAPH_BYTES};
use serde::{Serialize, Deserialize};

const MANAGED_EVENT_MAGIC: &[u8] = b"FLUX-GRAPH-1\0";

pub struct ManagedEventRegistration {
    pub type_path: fn() -> &'static str,
    pub subject: fn() -> Result<String>,
    pub encode: fn(&NetworkEvent) -> Result<GraphValue>,
    pub decode: fn(Id, GraphValue) -> Result<NetworkEvent>,
}

crate::schema::inventory::collect!(ManagedEventRegistration);

#[macro_export]
macro_rules! register_managed_event {
    ($event:ty, $catalog:path) => {
        $crate::schema::inventory::submit! {
            $crate::serialization::ManagedEventRegistration {
                type_path: <$event as $crate::prelude::TypePath>::type_path,
                subject: || Ok($catalog()?.graph_for::<$event>()?.root),
                encode: |event| $crate::serialization::encode_managed::<$event>(&$catalog()?, event),
                decode: |peer, value| $crate::serialization::decode_managed::<$event>(&$catalog()?, peer, value),
            }
        }
    };
}

pub fn encode_managed<T: Schema + Serialize + FromDynamic>(catalog: &Catalog, event: &NetworkEvent) -> Result<GraphValue> {
    let value = event.get_ev::<T>().context("Managed event does not match registered Rust type")?;
    let graph = catalog.graph_for::<T>()?;
    let value = serde_json::to_value(value)?;
    graph.validate_value(catalog, &value, ValueMode::Complete)?;
    Ok(GraphValue { format_version: crate::schema::FORMAT_VERSION, graph, value })
}

pub fn decode_managed<T: Schema + serde::de::DeserializeOwned + bevy_reflect::Struct>(catalog: &Catalog, peer: Id, value: GraphValue) -> Result<NetworkEvent> {
    let target = catalog.graph_for::<T>()?;
    let value = value.decode(catalog, &target, ValueMode::Complete)?;
    Ok(NetworkEvent::new(peer, serde_json::from_value::<T>(value)?))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedEventEnvelope {
    peer_id: Id,
    payload: GraphValue,
}

fn registration(event: &NetworkEvent) -> Result<Option<&'static ManagedEventRegistration>> {
    let type_info = event.ev.get_represented_type_info().context("Network event lacks represented type")?;
    let mut matches = crate::schema::inventory::iter::<ManagedEventRegistration>.into_iter()
        .filter(|registration| (registration.type_path)() == type_info.type_path());
    let registration = matches.next();
    ensure!(matches.next().is_none(), "Duplicate managed event registration");
    Ok(registration)
}

pub fn serialize_network_event(event: &NetworkEvent) -> Result<Vec<u8>> {
    if let Some(registration) = registration(event)? {
        let payload = (registration.encode)(event)?;
        let encoded = serde_json::to_vec(&ManagedEventEnvelope { peer_id: event.peer_id, payload })?;
        ensure!(encoded.len() + MANAGED_EVENT_MAGIC.len() <= MAX_GRAPH_BYTES, "Managed event exceeds byte limit");
        let mut bytes = MANAGED_EVENT_MAGIC.to_vec();
        bytes.extend(encoded);
        return Ok(bytes);
    }
    Ok(postcard::to_allocvec(event)?)
}

pub fn deserialize_network_event(bytes: &[u8]) -> Result<NetworkEvent> {
    if let Some(payload) = bytes.strip_prefix(MANAGED_EVENT_MAGIC) {
        ensure!(bytes.len() <= MAX_GRAPH_BYTES, "Managed event exceeds byte limit");
        let mut decoder = serde_json::Deserializer::from_slice(payload);
        let value = crate::schema::wire::unique_value(&mut decoder)?;
        decoder.end()?;
        let envelope: ManagedEventEnvelope = serde_json::from_value(value)?;
        let mut selected = None;
        for registration in crate::schema::inventory::iter::<ManagedEventRegistration> {
            if (registration.subject)()? == envelope.payload.graph.root {
                ensure!(selected.is_none(), "Duplicate managed subject registration");
                selected = Some(registration);
            }
        }
        return (selected.context("Unknown managed event subject")?.decode)(envelope.peer_id, envelope.payload);
    }
    let event = postcard::from_bytes(bytes)?;
    ensure!(registration(&event)?.is_none(), "Managed event requires a versioned contract envelope");
    Ok(event)
}
