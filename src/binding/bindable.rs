use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use crate::prelude::*;

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

#[derive(Component, Reflect, Reactive, Serialize, Deserialize, Clone, Debug)]
pub struct ReactiveView {
    pub id: Id
}