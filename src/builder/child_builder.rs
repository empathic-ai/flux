use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use bevy::utils::default;

use crate::prelude::*;

pub trait ChildTrait<'a> {
    fn child(&'a mut self)  -> EntityBuilder<'a>;
}

impl<'a> ChildTrait<'a> for ChildSpawnerCommands<'_> {
    fn child(&'a mut self) -> EntityBuilder<'a> {
        let entity_commands: EntityCommands<'_> = self.spawn(Control {
            ..default()
        });
        
        // Your implementation here
        EntityBuilder::new(entity_commands)
    }
}

pub trait EntityCommandsChildTrait<'a> {
    fn builder(self) -> EntityBuilder<'a>;
    //fn child(&'a mut self) -> EntityBuilder<'_>;
}

impl<'a> EntityCommandsChildTrait<'a> for EntityCommands<'a> {
    
    fn builder(self) -> EntityBuilder<'a> {
        // Your implementation here
        //let id = self.id();

        //let mut c: &mut Commands<'_, '_> = &mut ;
        //let c = &mut self.commands();
        //let commands = c.entity(id);
        EntityBuilder::new(self)
    }

    /*
    fn child(&'a mut self) -> EntityBuilder<'_> {
        // Your implementation here
        let id = self.id();
        let mut child_id: Option<Entity> = None;
        self.with_children(
            |parent| {
                child_id = Some(parent.child().id());
            }
        );
        //let mut c = self.commands();
        let commands = self.commands().entity(child_id.unwrap());

        self.add_child(child)
        EntityBuilder::from(commands)
    }
    */
}
pub trait CommandsChildTrait<'w, 's, 'a> {
    fn child(&'a mut self)  -> EntityBuilder<'a>;
}

impl<'w, 's, 'a> CommandsChildTrait<'w, 's, 'a> for Commands<'w, 's> {
    fn child(&'a mut self) -> EntityBuilder<'a> {
        let commands: EntityCommands<'_> = self.spawn(Control {..default()});
        EntityBuilder::new(commands)
    }
}

pub struct EntityChildSpawnerCommands<'a> {
    child_builder: &'a mut ChildSpawnerCommands<'a>,
}

impl<'a> EntityChildSpawnerCommands<'a> {
    pub fn new(child_builder: &'a mut ChildSpawnerCommands<'a>) -> Self {
        Self { 
            child_builder: child_builder, 
            //custom_steps: Vec::new(),
        }
    }
}