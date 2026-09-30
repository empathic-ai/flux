use super::{ExecuteError, Executor, ExecutorPlugin, TaskResult};
use crate::prelude::Id;
use bevy::prelude::*;
pub use bevy::prelude::{App, In, IntoSystem};
use std::{any::{Any, TypeId}, collections::HashMap, future::Future, sync::Arc};

pub trait ServiceRequest: Send + 'static {
    type Response: Send + 'static;
    type Error: Send + 'static;
}

#[derive(Clone, Copy, Debug)]
pub struct RequestContext {
    pub peer_id: Id,
    pub request_id: Id,
}

#[derive(Debug)]
pub enum CallError<E> {
    Remote(super::RpcError),
    Unregistered,
    Execution(ExecuteError),
    Application(E),
    InvalidResponse,
}

type Payload = Box<dyn Any + Send>;
type Handler = Arc<dyn Fn(Executor, RequestContext, Payload) -> TaskResult<Payload> + Send + Sync>;

/// Clones are registration snapshots. Construct clients after registering handlers.
#[derive(Resource, Clone, Default)]
pub struct ServiceRegistry {
    handlers: HashMap<TypeId, Handler>,
}

impl ServiceRegistry {
    pub fn call<Request: ServiceRequest>(
        &self, executor: Executor, context: RequestContext, request: Request,
    ) -> TaskResult<Request::Response, CallError<Request::Error>> {
        let handler = self.handlers.get(&TypeId::of::<Request>()).cloned();
        TaskResult::new(async move {
            let handler = handler.ok_or(CallError::Unregistered)?;
            let output = handler(executor, context, Box::new(request)).await
                .map_err(CallError::Execution)?;
            let result = output.downcast::<Result<Request::Response, Request::Error>>()
                .map_err(|_| CallError::InvalidResponse)?;
            (*result).map_err(CallError::Application)
        })
    }
}

pub trait Service {
    fn register(self, app: &mut App);
}

pub trait ServiceAppExt {
    fn register_service(&mut self, service: impl Service) -> &mut Self;
    fn register_system_handler<Request, HandlerSystem, Marker>(&mut self, system: HandlerSystem) -> &mut Self
    where
        Request: ServiceRequest,
        Request::Error: From<ExecuteError>,
        HandlerSystem: IntoSystem<In<(RequestContext, Request)>, TaskResult<Request::Response, Request::Error>, Marker>
            + Clone + Send + Sync + 'static;
    fn register_handler<Request, HandlerFn, HandlerFuture>(&mut self, handler: HandlerFn) -> &mut Self
    where
        Request: ServiceRequest,
        HandlerFn: Fn(Executor, RequestContext, Request) -> HandlerFuture + Send + Sync + 'static,
        HandlerFuture: Future<Output = Result<Request::Response, Request::Error>> + Send + 'static;
}

impl ServiceAppExt for App {
    fn register_system_handler<Request, HandlerSystem, Marker>(&mut self, system: HandlerSystem) -> &mut Self
    where
        Request: ServiceRequest,
        Request::Error: From<ExecuteError>,
        HandlerSystem: IntoSystem<In<(RequestContext, Request)>, TaskResult<Request::Response, Request::Error>, Marker>
            + Clone + Send + Sync + 'static,
    {
        self.register_handler::<Request, _, _>(move |executor, context, request| {
            let system = system.clone();
            async move {
                let task = executor.run_system(system, (context, request)).await
                    .map_err(Request::Error::from)?;
                task.await
            }
        })
    }

    fn register_service(&mut self, service: impl Service) -> &mut Self {
        service.register(self);
        self
    }

    fn register_handler<Request, HandlerFn, HandlerFuture>(&mut self, handler: HandlerFn) -> &mut Self
    where
        Request: ServiceRequest,
        HandlerFn: Fn(Executor, RequestContext, Request) -> HandlerFuture + Send + Sync + 'static,
        HandlerFuture: Future<Output = Result<Request::Response, Request::Error>> + Send + 'static,
    {
        if !self.is_plugin_added::<ExecutorPlugin>() {
            self.add_plugins(ExecutorPlugin);
        }
        self.init_resource::<ServiceRegistry>();
        let handler = Arc::new(handler);
        let erased: Handler = Arc::new(move |executor, context, request| {
            let handler = handler.clone();
            TaskResult::new(async move {
                let request = request.downcast::<Request>()
                    .map_err(|_| ExecuteError::System("Invalid service request type".into()))?;
                Ok(Box::new(handler(executor, context, *request).await) as Payload)
            })
        });
        let mut registry = self.world_mut().resource_mut::<ServiceRegistry>();
        assert!(!registry.handlers.contains_key(&TypeId::of::<Request>()), "Service request already registered");
        registry.handlers.insert(TypeId::of::<Request>(), erased);
        self
    }
}