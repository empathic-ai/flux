use crate::*;
use bevy::prelude::*;

#[derive(Clone, Component)]
pub struct AutoBindableProperty {
    pub entity: Entity,
    pub property_name: String,
    pub entity_func: Option<SetPropertyFunc>
}