use anyhow::Result;
use flux::prelude::*;

pub fn serialize_network_event(event: &NetworkEvent) -> Result<Vec<u8>> {
    Ok(postcard::to_allocvec(event)?)
}

pub fn deserialize_network_event(bytes: &[u8]) -> Result<NetworkEvent> {
    Ok(postcard::from_bytes(bytes)?)
}