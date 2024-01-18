use bevy::prelude::*;
use crate::prelude::*;

#[derive(Component, Clone, Default, Reflect)]
pub struct InteractState {
    //pub image: String,
    pub is_hovering: bool,
    pub is_pressing: bool,
    pub is_focused: bool
    //pub is_right_click: bool
}

impl Bindable for InteractState {
    fn get(&self) -> Box<dyn Reflect> {
        Box::new(self.clone())
    }
    fn set(&mut self, value: Box<dyn Reflect>) {
        self.apply(value.as_reflect());
    }
}