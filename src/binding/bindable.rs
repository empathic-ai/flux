use bevy::{prelude::*, reflect::{DynamicList, List, DynamicMap, Map}};
use serde::{Deserialize, Serialize};
use crate::prelude::*;
use smart_clone::SmartClone;
use derive_more::Debug;

#[bevy_trait_query::queryable]
#[reflect_trait]
pub trait Bindable {
    fn get(&self) -> Box<dyn Reflect>;
    fn set(&mut self, value: Box<dyn Reflect>);
}

#[derive(Component)]
pub struct AutoBindable {
    pub value: Box<dyn Reflect>
}

/// An owned presentation value of any reflected kind (including structs and collections).
#[derive(Component, Clone, Debug, Reflect, Reactive, Serialize, Deserialize)]
pub struct ReactiveView {
    pub value: Dynamic,
}

// TODO: Implement FromReflect for DynamicLists, similar to DynamicStructs
#[derive(Component, SmartClone, Debug, Reflect, Reactive, Serialize, Deserialize)]
pub struct ReactiveListView {
    #[clone(clone_with = "DynamicList::to_dynamic_list")]
    #[serde(with = "dynamic_list_serde")]
    pub value: DynamicList,
    #[reflect(ignore)]
    #[serde(skip)]
    #[debug(skip)]
    pub create_entity_func: Option<EntityFunc>,
}

/// Renders a map snapshot as children. Rows carry `ReactiveMapKey` and `ReactiveView`.
/// Unchanged entries retain their children; changed entries rerun the renderer.
/// Iteration order is unspecified.
#[derive(Component, SmartClone, Debug, Reflect, Reactive, Serialize, Deserialize)]
pub struct ReactiveMapView {
    #[clone(clone_with = "DynamicMap::to_dynamic_map")]
    #[serde(with = "dynamic_map_serde")]
    pub value: DynamicMap,
    #[reflect(ignore)]
    #[serde(skip)]
    #[debug(skip)]
    pub create_entity_func: Option<EntityFunc>,
}

/// The map key for a row; its value lives in `ReactiveView.value`.
#[derive(Component, Clone, Debug, Reflect, Reactive, Serialize, Deserialize)]
pub struct ReactiveMapKey {
    pub value: Dynamic,
}
