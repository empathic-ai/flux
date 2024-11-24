use bevy::prelude::*;

#[bevy_trait_query::queryable]
#[reflect_trait]
pub trait Reflectable: Reflect {
}

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