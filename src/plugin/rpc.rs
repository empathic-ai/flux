use super::{CallError, Executor, RequestContext, ServiceRegistry, ServiceRequest, TaskResult};
use crate::prelude::Id;
use bevy::prelude::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, future::Future, pin::Pin, sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}}, time::Duration};

pub const RPC_VERSION: u16 = 1;
pub const RPC_MAX_PAYLOAD: usize = 64 * 1024;
pub const RPC_MAX_TIMEOUT_MS: u32 = 30_000;

pub trait WireRequest: ServiceRequest + Serialize + DeserializeOwned {
    const OPERATION: &'static str;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcRequest {
    pub version: u16,
    pub request_id: Id,
    pub operation: String,
    pub timeout_ms: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RpcError {
    UnsupportedVersion,
    UnknownOperation,
    InvalidPayload,
    Overloaded,
    DeadlineExceeded,
    Cancelled,
    Disconnected,
    HandlerFailed,
    InvalidReply,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcReply {
    pub version: u16,
    pub request_id: Id,
    pub result: Result<Vec<u8>, RpcError>,
}

pub type RpcTimer = fn(Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;

pub trait RpcTransport: Send + Sync + 'static {
    fn exchange(&self, request: RpcRequest) -> TaskResult<RpcReply, RpcError>;
    fn cancel(&self, request_id: Id);
}

#[derive(Clone)]
pub struct RemoteServiceClient {
    transport: Arc<dyn RpcTransport>,
    timer: RpcTimer,
    timeout_ms: u32,
    in_flight: Arc<AtomicUsize>,
}

struct ClientPermit(Arc<AtomicUsize>);

impl Drop for ClientPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct CancelOnDrop {
    transport: Arc<dyn RpcTransport>,
    request_id: Id,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.transport.cancel(self.request_id);
        }
    }
}

impl RemoteServiceClient {
    pub fn new(transport: Arc<dyn RpcTransport>, timer: RpcTimer, timeout_ms: u32) -> Self {
        Self { transport, timer, timeout_ms: timeout_ms.clamp(1, RPC_MAX_TIMEOUT_MS), in_flight: Arc::new(AtomicUsize::new(0)) }
    }

    pub fn call<Request>(&self, request: Request) -> TaskResult<Request::Response, CallError<Request::Error>>
    where
        Request: WireRequest,
        Request::Response: DeserializeOwned,
        Request::Error: DeserializeOwned,
    {
        let client = self.clone();
        TaskResult::new(async move {
            client.in_flight.fetch_update(Ordering::AcqRel, Ordering::Acquire,
                |count| (count < 64).then_some(count + 1))
                .map_err(|_| CallError::Remote(RpcError::Overloaded))?;
            let _permit = ClientPermit(client.in_flight.clone());
            let payload = serde_json::to_vec(&request).map_err(|_| CallError::Remote(RpcError::InvalidPayload))?;
            if payload.len() > RPC_MAX_PAYLOAD {
                return Err(CallError::Remote(RpcError::InvalidPayload));
            }
            let request_id = Id::new();
            let envelope = RpcRequest {
                version: RPC_VERSION, request_id, operation: Request::OPERATION.into(),
                timeout_ms: client.timeout_ms, payload,
            };
            let mut cancel = CancelOnDrop { transport: client.transport.clone(), request_id, armed: true };
            let exchange = Box::pin(client.transport.exchange(envelope));
            let timeout = (client.timer)(Duration::from_millis(client.timeout_ms.into()));
            let reply = match futures::future::select(exchange, timeout).await {
                futures::future::Either::Left((reply, _)) => reply.map_err(CallError::Remote)?,
                futures::future::Either::Right(_) => return Err(CallError::Remote(RpcError::DeadlineExceeded)),
            };
            if reply.version != RPC_VERSION || reply.request_id != request_id {
                return Err(CallError::Remote(RpcError::InvalidReply));
            }
            cancel.armed = false;
            let payload = reply.result.map_err(CallError::Remote)?;
            if payload.len() > RPC_MAX_PAYLOAD {
                return Err(CallError::Remote(RpcError::InvalidPayload));
            }
            let result: Result<Request::Response, Request::Error> = serde_json::from_slice(&payload)
                .map_err(|_| CallError::Remote(RpcError::InvalidPayload))?;
            result.map_err(CallError::Application)
        })
    }
}

type ActiveCalls = Arc<Mutex<HashMap<(Id, Id), futures::future::AbortHandle>>>;

#[derive(Clone)]
pub struct RpcServer {
    wire: WireServiceRegistry,
    registry: ServiceRegistry,
    executor: Executor,
    timer: RpcTimer,
    active: ActiveCalls,
}

struct ServerPermit {
    active: ActiveCalls,
    key: (Id, Id),
}

impl Drop for ServerPermit {
    fn drop(&mut self) {
        self.active.lock().unwrap_or_else(|error| error.into_inner()).remove(&self.key);
    }
}

impl RpcServer {
    pub fn new(wire: WireServiceRegistry, registry: ServiceRegistry, executor: Executor, timer: RpcTimer) -> Self {
        Self { wire, registry, executor, timer, active: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn cancel(&self, authenticated_peer: Id, request_id: Id) {
        if let Some(handle) = self.active.lock().unwrap_or_else(|error| error.into_inner())
            .get(&(authenticated_peer, request_id)) {
            handle.abort();
        }
    }

    pub fn disconnect(&self, authenticated_peer: Id) {
        for ((peer, _), handle) in self.active.lock().unwrap_or_else(|error| error.into_inner()).iter() {
            if *peer == authenticated_peer { handle.abort(); }
        }
    }

    pub fn dispatch(&self, authenticated_peer: Id, request: RpcRequest) -> TaskResult<RpcReply, RpcError> {
        let server = self.clone();
        TaskResult::new(async move {
            let key = (authenticated_peer, request.request_id);
            let (abort, registration) = futures::future::AbortHandle::new_pair();
            {
                let mut active = server.active.lock().unwrap_or_else(|error| error.into_inner());
                if active.contains_key(&key) { return Err(RpcError::InvalidPayload); }
                if active.len() >= 256 || active.keys().filter(|(peer, _)| *peer == authenticated_peer).count() >= 32 {
                    return Err(RpcError::Overloaded);
                }
                active.insert(key, abort);
            }
            let _permit = ServerPermit { active: server.active.clone(), key };
            let task = server.wire.dispatch(server.registry, server.executor, authenticated_peer, request, server.timer);
            futures::future::Abortable::new(task, registration).await.map_err(|_| RpcError::Cancelled)?
        })
    }
}

type WireHandler = Arc<dyn Fn(ServiceRegistry, Executor, RequestContext, Vec<u8>)
    -> TaskResult<Vec<u8>, RpcError> + Send + Sync>;

#[derive(Resource, Clone, Default)]
pub struct WireServiceRegistry {
    handlers: HashMap<String, WireHandler>,
}

impl WireServiceRegistry {
    pub fn expose<Request>(&mut self)
    where
        Request: WireRequest,
        Request::Response: Serialize,
        Request::Error: Serialize,
    {
        assert!(!Request::OPERATION.is_empty(), "Wire operation ID cannot be empty");
        assert!(!self.handlers.contains_key(Request::OPERATION), "Duplicate wire operation ID");
        self.handlers.insert(Request::OPERATION.into(), Arc::new(|registry, executor, context, payload| {
            TaskResult::new(async move {
                let request = serde_json::from_slice::<Request>(&payload).map_err(|_| RpcError::InvalidPayload)?;
                let result = match registry.call(executor, context, request).await {
                    Ok(response) => Ok(response),
                    Err(CallError::Application(error)) => Err(error),
                    Err(CallError::Unregistered) => return Err(RpcError::UnknownOperation),
                    Err(_) => return Err(RpcError::HandlerFailed),
                };
                let payload = serde_json::to_vec(&result).map_err(|_| RpcError::HandlerFailed)?;
                if payload.len() > RPC_MAX_PAYLOAD { return Err(RpcError::InvalidPayload); }
                Ok(payload)
            })
        }));
    }

    pub fn dispatch(
        &self, registry: ServiceRegistry, executor: Executor, authenticated_peer: Id,
        request: RpcRequest, timer: RpcTimer,
    ) -> TaskResult<RpcReply, RpcError> {
        let handler = self.handlers.get(&request.operation).cloned();
        TaskResult::new(async move {
            let result = if request.version != RPC_VERSION {
                Err(RpcError::UnsupportedVersion)
            } else if request.payload.len() > RPC_MAX_PAYLOAD || request.timeout_ms == 0 || request.timeout_ms > RPC_MAX_TIMEOUT_MS {
                Err(RpcError::InvalidPayload)
            } else if let Some(handler) = handler {
                let task = Box::pin(handler(registry, executor, RequestContext {
                    peer_id: authenticated_peer, request_id: request.request_id,
                }, request.payload));
                match futures::future::select(task, timer(Duration::from_millis(request.timeout_ms.into()))).await {
                    futures::future::Either::Left((result, _)) => result,
                    futures::future::Either::Right(_) => Err(RpcError::DeadlineExceeded),
                }
            } else {
                Err(RpcError::UnknownOperation)
            };
            Ok(RpcReply { version: RPC_VERSION, request_id: request.request_id, result })
        })
    }
}