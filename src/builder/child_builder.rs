use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use bevy::utils::default;
use bevy::hierarchy::ChildBuilder;

use crate::prelude::*;

pub trait ChildTrait<'w, 's, 'a> {
    fn child(&'a mut self)  -> EntityBuilder<'w, 's, 'a>;
}

impl<'w, 's, 'a> ChildTrait<'w, 's, 'a> for ChildBuilder<'w, 's, '_> {
    fn child(&'a mut self) -> EntityBuilder<'w, 's, 'a> {
        let entity_commands: EntityCommands<'w, 's, '_> = self.spawn(Control {
            ..default()
        });
        
        // Your implementation here
        EntityBuilder::new(entity_commands)
    }
}

pub trait EntityCommandsChildTrait<'w, 's, 'a> {
    fn builder(&'a mut self)  -> EntityBuilder<'w, 's, 'a>;
    fn child(&'a mut self)  -> EntityBuilder<'w, 's, 'a>;
}

impl<'w, 's, 'a> EntityCommandsChildTrait<'w, 's, 'a> for EntityCommands<'w, 's, '_> {
    fn builder(&'a mut self) -> EntityBuilder<'w, 's, 'a> {
        // Your implementation here
        let id = self.id();
        let commands: EntityCommands<'w, 's, '_> = self.commands().entity(id);
        EntityBuilder::new(commands)
    }
    fn child(&'a mut self) -> EntityBuilder<'w, 's, 'a> {
        // Your implementation here
        let id = self.id();
        let mut child_id: Option<Entity> = None;
        self.with_children(
            |parent| {
                child_id = Some(parent.child().id());
            }
        );
        let commands = self.commands().entity(child_id.unwrap());
        EntityBuilder::new(commands)
    }
}
pub trait CommandsChildTrait<'w, 's, 'a> {
    fn child(&'a mut self)  -> EntityBuilder<'w, 's, 'a>;
}

impl<'w, 's, 'a> CommandsChildTrait<'w, 's, 'a> for Commands<'w, 's> {
    fn child(&'a mut self) -> EntityBuilder<'w, 's, 'a> {
        let commands: EntityCommands<'w, 's, '_> = self.spawn(Control {..default()});
        EntityBuilder::new(commands)
    }
}

pub struct EntityChildBuilder<'w, 's, 'a> {
    child_builder: &'a mut ChildBuilder<'w, 's, 'a>,
}

impl<'w, 's, 'a> EntityChildBuilder<'w, 's, 'a> {
    pub fn new(child_builder: &'a mut ChildBuilder<'w, 's, 'a>) -> Self {
        Self { 
            child_builder: child_builder, 
            //custom_steps: Vec::new(),
        }
    }
}