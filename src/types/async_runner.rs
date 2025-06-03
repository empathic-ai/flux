use bevy_async_ecs::AsyncWorld;

#[derive(Resource)]
pub struct AsyncRunner(AsyncWorld);

impl AsyncRunner {
    pub fn from_world(world: &mut World) -> Self {
        Self(AsyncWorld::from_world(world))
    }

    pub fn get_async_world(&self) -> AsyncWorld {
        self.0.clone()
    }
}