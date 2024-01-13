use bevy::prelude::*;

#[derive(Clone, Component)]
pub struct AutoBindableProperty {
    pub entity: Entity,
    pub property_name: String,
    entity_func: Option<SetPropertyFunc>
}