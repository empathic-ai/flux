use bevy::prelude::*;
use serde::*;

#[bevy_trait_query::queryable]
#[reflect_trait]
pub trait BindableList {
    fn get(&self) -> Box<dyn Reflect>;
    fn set(&self, value: &Box<&dyn Reflect>) {}
}

#[derive(Clone, Component, Serialize, Deserialize, Reflect)]
pub struct AutoBindableList {
    pub entity: Entity,
    pub property_name: String,
    #[reflect(ignore)]
    #[serde(skip)]
    create_entity: Option<CreateEntityFunc>
}

impl BindableList for AutoBindableList {
    fn get(&self) -> Box<dyn Reflect> {
        Box::new(self.clone())
    }

    fn set(&self, value: &Box<&dyn Reflect>) {
        //console::log!("SETTING LIST VALUE".to_string());
        //if let Some(comp) = value.downcast_ref::<T>() {
        //    console::log!("IS CHAT VIEW".to_string());
        //}
    }
}