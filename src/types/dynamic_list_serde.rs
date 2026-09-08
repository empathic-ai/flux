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
use std::{collections::BTreeMap, fmt};

pub fn serialize<S>(value: &DynamicList, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut registry = TypeRegistry::new();
    registry.register_global_types();
    let reflect_serializer = ReflectSerializer::new(value, &registry);

    reflect_serializer.serialize(serializer)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<DynamicList, D::Error>
where
    D: Deserializer<'de>,
{
    let mut registry = TypeRegistry::new();
    registry.register_global_types();
    let reflect_deserializer = ReflectDeserializer::new(&registry);
    let value = reflect_deserializer.deserialize(deserializer)?;

    if let ReflectRef::List(list_ref) = value.reflect_ref() {
        Ok(list_ref.to_dynamic_list())
    } else {
        Err(::serde::de::Error::custom("Value was not a dynamic list"))
    }
}