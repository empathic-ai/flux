use bevy_async_ecs::AsyncWorld;
use bevy::{ecs::system::SystemState, prelude::*};
use bevy_wasm_tasks::{JoinHandle, Tasks};

#[derive(Resource)]
pub struct AsyncRunner(AsyncWorld);

impl AsyncRunner {
    pub fn from_world(world: &mut World) -> Self {
 

        Self(AsyncWorld::from_world(world))
    }

    pub fn get_async_world(&self) -> AsyncWorld {
        self.0.clone()
    }

    pub fn run<Task, Output, Spawnable>(
        &self,
        task: Spawnable,
    )
    where
        Task: Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
        Spawnable: FnOnce(AsyncWorld) -> Task + Send + 'static,
    {
        let async_world = self.0.clone();
		self.0.apply(move |world: &mut World| {
            let mut system_state: SystemState<(Tasks)> = SystemState::new(world);
            let (tasks) = system_state.get_mut(world);
            
            tasks.spawn_auto(async move |_| {
                task(async_world).await;
            });
        });
    }
}