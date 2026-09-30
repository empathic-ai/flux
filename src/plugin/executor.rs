use bevy::{ecs::system::RunSystemOnce, prelude::*};
use std::{future::Future, pin::Pin, task::{Context, Poll}};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteError {
    Overloaded,
    Closed,
    System(String),
}

impl std::fmt::Display for ExecuteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for ExecuteError {}

/// Lazy owned work. Dropping it cancels pending work, not completed side effects.
#[must_use = "tasks do nothing unless polled or awaited"]
pub struct TaskResult<T, E = ExecuteError> {
    future: Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'static>>,
}

impl<T: Send + 'static, E: Send + 'static> TaskResult<T, E> {
    pub fn new(future: impl Future<Output = Result<T, E>> + Send + 'static) -> Self {
        Self { future: Box::pin(future) }
    }

    pub fn and_then<U: Send + 'static, F, Next>(self, next: F) -> TaskResult<U, E>
    where
        F: FnOnce(T) -> Next + Send + 'static,
        Next: Future<Output = Result<U, E>> + Send + 'static,
    {
        TaskResult::new(async move { next(self.await?).await })
    }
}

impl<T, E> Future for TaskResult<T, E> {
    type Output = Result<T, E>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.future.as_mut().poll(context)
    }
}

type Work = Box<dyn FnOnce(&mut World) + Send + 'static>;

#[derive(Resource, Clone)]
pub struct Executor {
    sender: async_channel::Sender<Work>,
}

#[derive(Resource)]
struct ExecutorQueue(async_channel::Receiver<Work>);

impl Drop for ExecutorQueue {
    fn drop(&mut self) {
        self.0.close();
        while self.0.try_recv().is_ok() {}
    }
}

impl Executor {
    /// Queue a one-shot system when polled. Each invocation has fresh `Local` state;
    /// persistent state belongs in resources, not event readers or system locals.
    pub fn run_system<Input, Output, Marker>(
        &self,
        system: impl IntoSystem<In<Input>, Output, Marker> + Send + 'static,
        input: Input,
    ) -> TaskResult<Output>
    where
        Input: Send + 'static,
        Output: Send + 'static,
    {
        let sender = self.sender.clone();
        TaskResult::new(async move {
            let (reply, result) = async_channel::bounded(1);
            let work: Work = Box::new(move |world| {
                if reply.is_closed() {
                    return;
                }
                let output = world.run_system_once_with(system, input)
                    .map_err(|error| ExecuteError::System(error.to_string()));
                let _ = reply.try_send(output);
            });
            sender.try_send(work).map_err(|error| match error {
                async_channel::TrySendError::Full(_) => ExecuteError::Overloaded,
                async_channel::TrySendError::Closed(_) => ExecuteError::Closed,
            })?;
            result.recv().await.map_err(|_| ExecuteError::Closed)?
        })
    }
}

pub struct ExecutorPlugin;

impl Plugin for ExecutorPlugin {
    fn build(&self, app: &mut App) {
        let (sender, receiver) = async_channel::bounded(256);
        app.insert_resource(Executor { sender })
            .insert_resource(ExecutorQueue(receiver))
            .add_systems(Last, execute_queued_systems);
    }
}

fn execute_queued_systems(world: &mut World) {
    let receiver = world.resource::<ExecutorQueue>().0.clone();
    for _ in 0..64 {
        let Ok(work) = receiver.try_recv() else { break; };
        work(world);
    }
}