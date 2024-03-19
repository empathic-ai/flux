use bevy::ecs::system::EntityCommands;

pub trait Builder<'a> : Sized {
    fn get_commands(&mut self) -> &mut EntityCommands<'a>;
}