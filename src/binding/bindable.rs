use bevy::prelude::*;

#[derive(Component)]
pub struct AutoBindable {
    value: Box<dyn Reflect>
}

impl AutoBindable {
}

impl Bindable for AutoBindable {
    fn get(&self) -> Box<dyn Reflect> {
        self.value.clone_value()
    }

    fn set(&mut self, value: Box<dyn Reflect>) {
        //let v = value.as_ref();
        self.value = value;
        //
    }
}