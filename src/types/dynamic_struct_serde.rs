use bevy::reflect::{DynamicStruct, DynamicTypePath, PartialReflect, Reflect, ReflectSerialize, Struct, TypeInfo, TypeRegistry};
use serde::{
    de::{Error as DeError, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{collections::BTreeMap, fmt};

/// Helper wrapper that calls into the field’s ReflectSerialize impl.
struct DynamicField<'a> {
    field: &'a dyn PartialReflect,
}

impl<'a> Serialize for DynamicField<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(reflect_ser) = self.field.serializable() {
            match reflect_ser {
                bevy::reflect::serde::Serializable::Owned(serialize) => {
                    serialize.serialize(serializer)
                },
                bevy::reflect::serde::Serializable::Borrowed(serialize) => {
                    serialize.serialize(serializer)
                },
            }
        } else {
            Err(serde::ser::Error::custom(
                "Field does not support ReflectSerialize",
            ))
        }
    }
}

/// The module you can refer to with #[serde(with = "dynamic_struct_serde")]
//pub mod dynamic_struct_serde {
//    use super::*;

    /// Serializes a DynamicStruct as a map with two entries:
    /// - "type": the concrete type name (if known)
    /// - "fields": a map of field names to field values.
    pub fn serialize<S>(value: &DynamicStruct, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        // Include the concrete type name (if available)

        if let Some(type_info) = value.get_represented_type_info() {
            map.serialize_entry("type", type_info.type_path())?;
        } else {
            map.serialize_entry("type", value.reflect_type_path())?;
        }
  
        // Create a temporary map for fields.
        let mut fields_map = BTreeMap::new();
        for i in 0..value.field_len() {
            let field_name = value.name_at(i).expect("Failed to find field name during dynamic type serialization.");
            let field = value.field(field_name).ok_or_else(|| {
                serde::ser::Error::custom(format!("Missing field '{}'", field_name))
            })?;
            fields_map.insert(field_name.to_string(), DynamicField { field });
        }
        map.serialize_entry("fields", &fields_map)?;
        map.end()
    }

    /// For deserialization we expect the same format that we serialized:
    /// a map with "type" and "fields".
    #[derive(Deserialize)]
    struct DynamicStructRepr {
        #[serde(rename = "type")]
        type_name: String,
        fields: BTreeMap<String, serde_json::Value>,
    }

    /// Note: Deserialization requires that the concrete type be registered.
    /// You might want to pass in your TypeRegistry instead of using a default.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DynamicStruct, D::Error>
    where
        D: Deserializer<'de>,
    {

        todo!()
        /*
        // Deserialize into an intermediate representation.
        let repr = DynamicStructRepr::deserialize(deserializer)?;

        // Obtain the type registration.
        // In a real application you’d likely pass a registry instance
        // rather than using a default.
        let registry = TypeRegistry::default();
        let registration = registry.get_with_name(&repr.type_name).ok_or_else(|| {
            D::Error::custom(format!("Unknown type: {}", repr.type_name))
        })?;

        // Make sure the registered type is a struct.
        let struct_registration = registration
            .data::<Struct>()
            .ok_or_else(|| D::Error::custom("Registered type is not a struct"))?;

        // Create a new DynamicStruct.
        // (This requires that DynamicStruct have a way to create an empty instance;
        // adjust if your API is different.)
        let mut dynamic = DynamicStruct::default();

        // For each field in the serialized map, look up the field info from the registration,
        // then deserialize the value into the field’s type.
        // (Here we use serde_json as an intermediate representation. In a full solution you
        // might integrate with the ReflectDeserialize API instead.)
        for (field_name, value) in repr.fields {
            let field_info = struct_registration.field(field_name.as_str()).ok_or_else(|| {
                D::Error::custom(format!(
                    "Field '{}' not found in type {}",
                    field_name, repr.type_name
                ))
            })?;

            // Deserialize the field value.
            // This assumes that the field’s type implements Deserialize.
            let deserialized_field = serde_json::from_value(value)
                .map_err(|e| D::Error::custom(format!("Error deserializing field {}: {}", field_name, e)))?;

            // Insert the field into the dynamic struct.
            // (Assuming an `insert` method exists; adjust to your API.)
            dynamic
                .insert(field_name, deserialized_field);
                //.map_err(|e| D::Error::custom(format!("Error inserting field: {}", e)))?;
        }
        Ok(dynamic)
         */
    }
//}
