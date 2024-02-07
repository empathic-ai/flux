use crate::prelude::*;
use bevy::prelude::*;

#[derive(Clone, Component)]
pub struct AutoBindableProperty {
    pub entity: Entity,
    pub component_name: String,
    pub property_name: String,
    pub entity_func: Option<SetPropertyFunc>
}

#[derive(Clone, Component)]
pub struct PropertyBinder {
    pub property_path_parts: Vec<String>,
    pub property_entities: Vec<Option<String>>,
    pub entity_func: Option<SetPropertyFunc>
}