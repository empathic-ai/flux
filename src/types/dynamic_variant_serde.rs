use bevy::{prelude::info, reflect::{serde::{ReflectDeserializer, ReflectSerializer}, DynamicStruct, DynamicTypePath, DynamicVariant, PartialReflect, Reflect, ReflectRef, ReflectSerialize, Struct, TypeInfo, TypeRegistry}, scene::ron};
use serde::{
de::{Error as DeError, MapAccess, Visitor},
ser::SerializeMap,
Deserialize, Deserializer, Serialize, Serializer,
};
use std::{collections::BTreeMap, fmt};
use reflect_steroids::prelude::*;
use serde::de::DeserializeSeed;

/// Serializes a DynamicStruct as a map with two entries:
/// - "type": the concrete type name (if known)
/// - "fields": a map of field names to field values.
pub fn serialize<S>(value: &DynamicVariant, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    //info!("Using dynamic struct serializer.");

    let mut registry = TypeRegistry::new();
    registry.register_global_types();
    let reflect_deserializer = ReflectSerializer::new(value, &registry);

    reflect_deserializer.serialize(serializer)
}

/// Note: Deserialization requires that the concrete type be registered.
/// You might want to pass in your TypeRegistry instead of using a default.
pub fn deserialize<'de, D>(deserializer: D) -> Result<DynamicVariant, D::Error>
where
    D: Deserializer<'de>,
{
    let mut registry = TypeRegistry::new();
    registry.register_global_types();
    let reflect_deserializer = ReflectDeserializer::new(&registry);
    let value = reflect_deserializer.deserialize(deserializer)?;

    //info!("Using dynamic struct deserializer.");

    if let ReflectRef::Variant(variant_ref) = value.reflect_ref() {
        Ok(struct_ref.clone_dynamic())
    } else {
        Err(serde::de::Error::custom("Value was not a dynamic struct"))
    }

    /*
    //info!("Is dynamic: {}", result.is_dynamic());
    //info!("Represented type: {}", result.get_represented_type_info().unwrap().type_path());
    let mut dynamic_struct = DynamicStruct::default();
    dynamic_struct.set_represented_type(value.get_represented_type_info());
    dynamic_struct.apply(value.as_partial_reflect());
    //info!("Represented type: {}", dynamic_struct.get_represented_type_info().unwrap().type_path());
    Ok(dynamic_struct)
    */

    /*
    match result.try_downcast_ref::<DynamicStruct>() {
        Some(result) => {
            Ok(result.clone_dynamic())
        },
        None => {
            info!("Failed to deserialize value! Value was not a dynamic struct.");
            Err(serde::de::Error::custom("Value was not a dynamic struct"))
        }
    } */
}
