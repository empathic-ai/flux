use crate::prelude::Dynamic;
use ::serde::{Deserializer, Serializer};
use bevy_reflect::{DynamicMap, ReflectRef};

pub fn serialize<S: Serializer>(value: &DynamicMap, serializer: S) -> Result<S::Ok, S::Error> {
    ::serde::Serialize::serialize(&Dynamic::new(value), serializer)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DynamicMap, D::Error> {
    let value: Dynamic = ::serde::Deserialize::deserialize(deserializer)?;
    match value.as_ref().reflect_ref() {
        ReflectRef::Map(map) => Ok(map.to_dynamic_map()),
        _ => Err(::serde::de::Error::custom("Value was not a reflected map")),
    }
}
