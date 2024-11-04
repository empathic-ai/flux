pub mod builder;
pub mod child_builder;
pub mod entity_builder;

pub use builder::*;
pub use child_builder::*;
pub use entity_builder::*;

use bevy::prelude::*;
use bevy::reflect::{List, DynamicList, DynamicStruct};
use std::collections::HashMap;

// TODO: Either remove or uncomment
// Currently not in use but may be useful for converting Reflect types to JSON without using TypeRegistry
/*
pub fn reflect_to_string(reflect: &dyn Reflect) -> String {
    reflect_to_json(reflect).to_string()
}

pub fn reflect_to_json(reflect: &dyn Reflect) -> serde_json::Value {
    if let Some(value) = reflect.downcast_ref::<i32>() {
        json!(value)
    } else if let Some(value) = reflect.downcast_ref::<f32>() {
        json!(value)
    } else if let Some(value) = reflect.downcast_ref::<String>() {
        json!(value)
    } else if let Some(value) = reflect.downcast_ref::<bool>() {
        json!(value)
    } else if let Some(list) = reflect.downcast_ref::<DynamicList>() {
        let json_list: Vec<_> = (0..list.len()).map(|i| reflect_to_json(list.get(i).unwrap())).collect();
        json!(json_list)
    } else if let Some(structure) = reflect.downcast_ref::<DynamicStruct>() {
        let mut json_map = HashMap::new();
        for i in 0..structure.field_len() {
            json_map.insert(structure.name_at(i).unwrap(), reflect_to_json(structure.field_at(i).unwrap()));
        }
        json!(json_map)
    } else {
        json!(null) // Handle unsupported types as null or throw an error
    }
}
*/