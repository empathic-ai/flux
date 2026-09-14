use bevy_reflect::{
    *, serde::{ReflectDeserializer, ReflectSerializer},
};
use reflect_steroids::prelude::*;
use ::serde::de::DeserializeSeed;
use ::serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as DeError, MapAccess, Visitor},
    ser::SerializeMap,
};
use std::{collections::{BTreeMap, HashMap}, fmt};

fn register_common_types(registry: &mut TypeRegistry) {
    registry.register_global_types();
    registry.register::<String>();
    registry.register::<Option<String>>();
    registry.register::<Vec<String>>();
    registry.register::<BTreeMap<String, String>>();
    registry.register::<HashMap<String, String>>();
    registry.register::<uuid::Uuid>();
}

pub fn serialize<S>(value: &DynamicStruct, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    //info!("Using dynamic struct serializer.");

    let mut registry = TypeRegistry::new();
    register_common_types(&mut registry);
    let reflect_deserializer = ReflectSerializer::new(value, &registry);

    reflect_deserializer.serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<DynamicStruct, D::Error>
where
    D: Deserializer<'de>,
{
    let mut registry = TypeRegistry::new();
    register_common_types(&mut registry);
    let reflect_deserializer = ReflectDeserializer::new(&registry);
    let value = reflect_deserializer.deserialize(deserializer)?;

    //info!("Using dynamic struct deserializer.");

    if let ReflectRef::Struct(struct_ref) = value.reflect_ref() {
        Ok(struct_ref.clone_dynamic())
    } else {
        Err(::serde::de::Error::custom("Value was not a dynamic struct"))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde::{Deserialize, Serialize};
    use smart_clone::SmartClone;

    use crate::prelude::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize, Reflect, SmartClone)]
    struct WifiConfigEvent {
        wifi_configs: HashMap<String, String>,
    }

    #[test]
    fn network_event_serializes_hash_map_payloads() {
        let ev = NetworkEvent::new(
            Id::nil(),
            WifiConfigEvent {
                wifi_configs: HashMap::from([
                    ("Home WiFi".to_string(), "secret".to_string()),
                    ("Office".to_string(), "pass".to_string()),
                ]),
            },
        );

        let bytes = postcard::to_allocvec(&ev).expect("network event should serialize");
        let decoded = postcard::from_bytes::<NetworkEvent>(&bytes)
            .expect("network event should deserialize");

        let payload = decoded
            .get_ev::<WifiConfigEvent>()
            .expect("event payload should be present");

        assert_eq!(payload.wifi_configs.get("Home WiFi"), Some(&"secret".to_string()));
    }
}
