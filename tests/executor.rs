#![cfg(all(feature = "bevy_std", feature = "futures"))]

use bevy::prelude::*;
use flux::prelude::*;
use std::{future::Future, pin::Pin, task::{Context, Poll}};

fn poll_task<T>(task: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
    task.poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
}

#[derive(Resource, Default)]
struct Count(u32);

fn add(In(amount): In<u32>, mut count: ResMut<Count>) -> u32 {
    count.0 += amount;
    count.0
}

struct AddRequest(u32);

impl ServiceRequest for AddRequest {
    type Response = u32;
    type Error = ExecuteError;
}

struct CounterService;

struct EchoRequest(u32);
struct DoubleRequest(u32);

#[service]
trait ExampleApi {
    async fn echo(request: EchoRequest) -> Result<u32, ExecuteError>;
    #[system]
    async fn double(request: DoubleRequest) -> Result<u32, ExecuteError>;
}

fn double_handler(In((_context, request)): In<(RequestContext, DoubleRequest)>) -> TaskResult<u32> {
    TaskResult::new(async move { Ok(request.0 * 2) })
}

#[test]
fn generated_client_dispatches_async_and_system_methods() {
    let mut app = App::new();
    app.register_service(ExampleApi::handlers(
        |_executor, _context, request: EchoRequest| async move { Ok(request.0) },
        double_handler,
    ));
    let client = ExampleApiClient::new(
        app.world().resource::<ServiceRegistry>().clone(),
        app.world().resource::<Executor>().clone(),
        Id::new(),
    );
    let mut echo = Box::pin(client.echo(EchoRequest(3)));
    assert!(matches!(poll_task(echo.as_mut()), Poll::Ready(Ok(3))));
    let mut double = Box::pin(client.double(DoubleRequest(4)));
    assert!(poll_task(double.as_mut()).is_pending());
    app.update();
    assert!(matches!(poll_task(double.as_mut()), Poll::Ready(Ok(8))));
}

fn deferred_add(
    In((_context, request)): In<(RequestContext, AddRequest)>,
    executor: Res<Executor>,
) -> TaskResult<u32> {
    executor.run_system(add, request.0)
        .and_then(|value| async move { Ok(value * 2) })
}

#[test]
fn system_handler_returns_a_deferred_chain() {
    let mut app = App::new();
    app.init_resource::<Count>()
        .register_system_handler::<AddRequest, _, _>(deferred_add);
    let registry = app.world().resource::<ServiceRegistry>().clone();
    let executor = app.world().resource::<Executor>().clone();
    let context = RequestContext { peer_id: Id::new(), request_id: Id::new() };
    let mut call = Box::pin(registry.call(executor, context, AddRequest(7)));
    assert!(poll_task(call.as_mut()).is_pending());
    app.update();
    assert!(poll_task(call.as_mut()).is_pending());
    app.update();
    assert!(matches!(poll_task(call.as_mut()), Poll::Ready(Ok(14))));
}

impl Service for CounterService {
    fn register(self, app: &mut App) {
        app.register_handler::<AddRequest, _, _>(|executor, _context, request| async move {
            executor.run_system(add, request.0).await
        });
    }
}

#[test]
fn registered_service_returns_ecs_result() {
    let mut app = App::new();
    app.init_resource::<Count>().register_service(CounterService);
    let registry = app.world().resource::<ServiceRegistry>().clone();
    let executor = app.world().resource::<Executor>().clone();
    let context = RequestContext { peer_id: Id::new(), request_id: Id::new() };
    let mut call = Box::pin(registry.call(executor, context, AddRequest(7)));
    assert!(poll_task(call.as_mut()).is_pending());
    app.update();
    assert!(matches!(poll_task(call.as_mut()), Poll::Ready(Ok(7))));
}

#[test]
fn missing_service_fails_without_queueing_work() {
    let mut app = App::new();
    app.add_plugins(ExecutorPlugin);
    let executor = app.world().resource::<Executor>().clone();
    let registry = ServiceRegistry::default();
    let context = RequestContext { peer_id: Id::new(), request_id: Id::new() };
    let mut call = Box::pin(registry.call(executor, context, AddRequest(7)));
    assert!(matches!(poll_task(call.as_mut()), Poll::Ready(Err(CallError::Unregistered))));
}

#[test]
fn typed_chain_returns_final_system_output() {
    let mut app = App::new();
    app.add_plugins(ExecutorPlugin).init_resource::<Count>();
    let executor = app.world().resource::<Executor>().clone();
    let next = executor.clone();
    let mut task = Box::pin(executor.run_system(add, 2)
        .and_then(move |value| next.run_system(add, value + 1)));
    assert!(poll_task(task.as_mut()).is_pending());
    app.update();
    assert!(poll_task(task.as_mut()).is_pending());
    app.update();
    assert_eq!(poll_task(task.as_mut()), Poll::Ready(Ok(5)));
}

#[test]
fn dropping_pending_task_skips_unstarted_mutation() {
    let mut app = App::new();
    app.add_plugins(ExecutorPlugin).init_resource::<Count>();
    let executor = app.world().resource::<Executor>().clone();
    let mut task = Box::pin(executor.run_system(add, 2));
    assert!(poll_task(task.as_mut()).is_pending());
    drop(task);
    app.update();
    assert_eq!(app.world().resource::<Count>().0, 0);
}

#[test]
fn closed_world_fails_pending_and_new_tasks() {
    let mut app = App::new();
    app.add_plugins(ExecutorPlugin).init_resource::<Count>();
    let executor = app.world().resource::<Executor>().clone();
    let mut pending = Box::pin(executor.run_system(add, 2));
    assert!(poll_task(pending.as_mut()).is_pending());
    drop(app);
    assert_eq!(poll_task(pending.as_mut()), Poll::Ready(Err(ExecuteError::Closed)));
    let mut next = Box::pin(executor.run_system(add, 3));
    assert_eq!(poll_task(next.as_mut()), Poll::Ready(Err(ExecuteError::Closed)));
}

#[test]
fn queue_overload_is_explicit() {
    let mut app = App::new();
    app.add_plugins(ExecutorPlugin).init_resource::<Count>();
    let executor = app.world().resource::<Executor>().clone();
    let mut tasks = Vec::new();
    for _ in 0..256 {
        let mut task = Box::pin(executor.run_system(add, 1));
        assert!(poll_task(task.as_mut()).is_pending());
        tasks.push(task);
    }
    let mut overflow = Box::pin(executor.run_system(add, 1));
    assert_eq!(poll_task(overflow.as_mut()), Poll::Ready(Err(ExecuteError::Overloaded)));
}