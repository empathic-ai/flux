use crate::types::Id;
use serde::{Deserialize, Serialize};

pub const RPC_VERSION: u16 = 1;
pub const RPC_MAX_PAYLOAD: usize = 64 * 1024;
pub const RPC_MAX_TIMEOUT_MS: u32 = 30_000;
const MAX_FRAME_BYTES: usize = RPC_MAX_PAYLOAD * 4 + 8192;

pub trait RpcConnectionAdapter: Send + Sync + 'static {
    fn authenticated_peer(&self) -> Id;
    fn max_message_bytes(&self) -> usize;
    fn send(&self, frame: Vec<u8>) -> Result<(), RpcError>;
    fn close(&self) {}
}

pub type RpcDispatch = std::pin::Pin<Box<dyn std::future::Future<Output = Result<RpcReply, RpcError>> + Send>>;

pub trait RpcDispatcher: Send + Sync + 'static {
    fn dispatch(&self, authenticated_peer: Id, request: RpcRequest) -> RpcDispatch;
    /// Capture admission state before transport queues defer polling the request.
    fn prepare(self: std::sync::Arc<Self>, authenticated_peer: Id, request: RpcRequest) -> RpcDispatch {
        Box::pin(async move { self.dispatch(authenticated_peer, request).await })
    }
    fn disconnect(&self, _connection: Id) {}
}

pub trait RpcAuthorizer: Send + Sync + 'static {
    fn authorize(&self, connection: Id, operation: &str) -> Result<(), RpcError>;
    fn revoked(&self) -> RpcRevocation {
        Box::pin(std::future::pending())
    }
}

pub type RpcRevocation = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
type AuthorityWaiter = std::sync::Mutex<Option<std::task::Waker>>;

struct RpcAuthorityState {
    active: std::sync::atomic::AtomicBool,
    waiters: std::sync::Mutex<Vec<std::sync::Weak<AuthorityWaiter>>>,
    parent: Option<RpcAuthorityPermit>,
}

pub struct RpcAuthorityScope {
    active: std::sync::Arc<RpcAuthorityState>,
}

#[derive(Clone)]
pub struct RpcAuthorityPermit {
    active: std::sync::Arc<RpcAuthorityState>,
}

impl Default for RpcAuthorityScope {
    fn default() -> Self {
        Self { active: std::sync::Arc::new(RpcAuthorityState {
            active: std::sync::atomic::AtomicBool::new(true),
            waiters: std::sync::Mutex::new(Vec::new()),
            parent: None,
        }) }
    }
}

impl RpcAuthorityScope {
    pub fn permit(&self) -> RpcAuthorityPermit {
        RpcAuthorityPermit { active: self.active.clone() }
    }

    pub fn retire(&self) {
        self.active.active.store(false, std::sync::atomic::Ordering::Release);
        let waiters = std::mem::take(&mut *self.active.waiters.lock().unwrap_or_else(|error| error.into_inner()));
        for waiter in waiters.into_iter().filter_map(|waiter| waiter.upgrade()) {
            let waker = waiter.lock().unwrap_or_else(|error| error.into_inner()).take();
            if let Some(waker) = waker { waker.wake(); }
        }
    }
}

impl Drop for RpcAuthorityScope {
    fn drop(&mut self) { self.retire(); }
}

impl RpcAuthorityPermit {
    pub fn child_scope(&self) -> RpcAuthorityScope {
        RpcAuthorityScope { active: std::sync::Arc::new(RpcAuthorityState {
            active: std::sync::atomic::AtomicBool::new(true),
            waiters: std::sync::Mutex::new(Vec::new()),
            parent: Some(self.clone()),
        }) }
    }

    pub fn check(&self) -> Result<(), RpcError> {
        if !self.active.active.load(std::sync::atomic::Ordering::Acquire) {
            return Err(RpcError::Unauthenticated);
        }
        match &self.active.parent {
            Some(parent) => parent.check(),
            None => Ok(()),
        }
    }

    pub fn revoked(&self) -> RpcRevocation {
        let permit = self.clone();
        let mut parent = self.active.parent.as_ref().map(Self::revoked);
        let waiter = std::sync::Arc::new(std::sync::Mutex::new(None::<std::task::Waker>));
        {
            let mut waiters = self.active.waiters.lock().unwrap_or_else(|error| error.into_inner());
            waiters.retain(|entry| entry.strong_count() != 0);
            waiters.push(std::sync::Arc::downgrade(&waiter));
        }
        Box::pin(std::future::poll_fn(move |context| {
            if parent.as_mut().is_some_and(|future| future.as_mut().poll(context).is_ready()) {
                return std::task::Poll::Ready(());
            }
            *waiter.lock().unwrap_or_else(|error| error.into_inner()) = Some(context.waker().clone());
            if permit.check().is_err() { std::task::Poll::Ready(()) }
            else { std::task::Poll::Pending }
        }))
    }
}

pub struct ScopedRpcAuthorizer {
    permit: RpcAuthorityPermit,
    policy: std::sync::Arc<dyn RpcAuthorizer>,
}

impl ScopedRpcAuthorizer {
    pub fn new(permit: RpcAuthorityPermit, policy: std::sync::Arc<dyn RpcAuthorizer>) -> Self {
        Self { permit, policy }
    }
}

impl RpcAuthorizer for ScopedRpcAuthorizer {
    fn revoked(&self) -> RpcRevocation {
        let mut scope = self.permit.revoked();
        let mut policy = self.policy.revoked();
        Box::pin(std::future::poll_fn(move |context| {
            if scope.as_mut().poll(context).is_ready() || policy.as_mut().poll(context).is_ready() {
                std::task::Poll::Ready(())
            } else { std::task::Poll::Pending }
        }))
    }

    fn authorize(&self, connection: Id, operation: &str) -> Result<(), RpcError> {
        self.permit.check()?;
        self.policy.authorize(connection, operation)?;
        self.permit.check()
    }
}

pub struct AuthorizedRpcDispatcher {
    dispatcher: std::sync::Arc<dyn RpcDispatcher>,
    authorizer: std::sync::Arc<dyn RpcAuthorizer>,
}

impl AuthorizedRpcDispatcher {
    pub fn new(dispatcher: std::sync::Arc<dyn RpcDispatcher>, authorizer: std::sync::Arc<dyn RpcAuthorizer>) -> Self {
        Self { dispatcher, authorizer }
    }
}

impl RpcDispatcher for AuthorizedRpcDispatcher {
    fn prepare(self: std::sync::Arc<Self>, connection: Id, request: RpcRequest) -> RpcDispatch {
        self.dispatch(connection, request)
    }
    fn disconnect(&self, connection: Id) { self.dispatcher.disconnect(connection); }

    fn dispatch(&self, connection: Id, request: RpcRequest) -> RpcDispatch {
        let dispatcher = self.dispatcher.clone();
        let authorizer = self.authorizer.clone();
        let admitted = authorizer.authorize(connection, &request.operation);
        let request_id = request.request_id;
        let operation = request.operation.clone();
        let prepared = admitted.as_ref().ok().map(|_| dispatcher.prepare(connection, request));
        Box::pin(async move {
            match admitted.and_then(|_| authorizer.authorize(connection, &operation)) {
                Ok(()) => {
                    let mut prepared = prepared.expect("admitted request has prepared dispatch");
                    let mut revoked = authorizer.revoked();
                    std::future::poll_fn(move |context| {
                        if revoked.as_mut().poll(context).is_ready() {
                            return std::task::Poll::Ready(Ok(RpcReply {
                                version: RPC_VERSION, request_id, result: Err(RpcError::Unauthenticated),
                            }));
                        }
                        if let Err(error) = authorizer.authorize(connection, &operation) {
                            return std::task::Poll::Ready(Ok(RpcReply {
                                version: RPC_VERSION, request_id, result: Err(error),
                            }));
                        }
                        match prepared.as_mut().poll(context) {
                            std::task::Poll::Ready(reply) => {
                                std::task::Poll::Ready(match authorizer.authorize(connection, &operation) {
                                    Ok(()) => reply,
                                    Err(error) => Ok(RpcReply { version: RPC_VERSION, request_id, result: Err(error) }),
                                })
                            }
                            std::task::Poll::Pending => std::task::Poll::Pending,
                        }
                    }).await
                }
                Err(error) => Ok(RpcReply { version: RPC_VERSION, request_id, result: Err(error) }),
            }
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcContractCall<Call> {
    pub call: Call,
}

pub fn decode_service_request<Request, Contracts>(contracts: &Contracts, payload: &[u8])
    -> Result<(Contracts::Call, Request), RpcError>
where
    Request: serde::de::DeserializeOwned,
    Contracts: crate::schema::wire::ServiceContractSet,
{
    if payload.len() > RPC_MAX_PAYLOAD { return Err(RpcError::InvalidPayload); }
    let mut decoder = serde_json::Deserializer::from_slice(payload);
    let value = crate::schema::wire::unique_value(&mut decoder).map_err(|_| RpcError::InvalidPayload)?;
    decoder.end().map_err(|_| RpcError::InvalidPayload)?;
    let envelope: RpcContractCall<Contracts::Call> = serde_json::from_value(value).map_err(|_| RpcError::InvalidPayload)?;
    let value = contracts.decode_request(&envelope.call).map_err(|_| RpcError::InvalidPayload)?;
    let request = serde_json::from_value(value).map_err(|_| RpcError::InvalidPayload)?;
    Ok((envelope.call, request))
}

pub fn encode_service_reply<Response, Error, Contracts>(contracts: &Contracts, call: &Contracts::Call,
    result: Result<Response, Error>) -> Result<Vec<u8>, RpcError>
where
    Response: Serialize,
    Error: Serialize,
    Contracts: crate::schema::wire::ServiceContractSet,
{
    let result = match result {
        Ok(response) => Ok(contracts.encode_response(call, serde_json::to_value(response)
            .map_err(|_| RpcError::HandlerFailed)?).map_err(|_| RpcError::HandlerFailed)?),
        Err(error) => Err(contracts.encode_error(call, serde_json::to_value(error)
            .map_err(|_| RpcError::HandlerFailed)?).map_err(|_| RpcError::HandlerFailed)?),
    };
    let payload = serde_json::to_vec(&result).map_err(|_| RpcError::HandlerFailed)?;
    if payload.len() > RPC_MAX_PAYLOAD { return Err(RpcError::InvalidPayload); }
    Ok(payload)
}

pub struct QueuedRpcAdapter {
    peer: Id,
    max_message_bytes: usize,
    max_pending: usize,
    queue: std::sync::Mutex<Option<std::collections::VecDeque<Vec<u8>>>>,
}

impl QueuedRpcAdapter {
    pub fn new(peer: Id, max_message_bytes: usize, max_pending: usize) -> Result<Self, RpcError> {
        let minimum = PeerRpcFrame::Reply(RpcReply {
            version: RPC_VERSION, request_id: Id::new(), result: Err(RpcError::UnsupportedVersion),
        }).encode()?.len();
        if max_message_bytes < minimum || max_message_bytes > MAX_FRAME_BYTES || max_pending == 0 || max_pending > 64 {
            return Err(RpcError::InvalidPayload);
        }
        Ok(Self { peer, max_message_bytes, max_pending, queue: std::sync::Mutex::new(Some(Default::default())) })
    }

    pub fn try_recv(&self) -> Result<Option<Vec<u8>>, RpcError> {
        self.queue.lock().unwrap_or_else(|error| error.into_inner())
            .as_mut().ok_or(RpcError::Disconnected).map(|queue| queue.pop_front())
    }

    pub fn close(&self) {
        *self.queue.lock().unwrap_or_else(|error| error.into_inner()) = None;
    }
}

impl RpcConnectionAdapter for QueuedRpcAdapter {
    fn authenticated_peer(&self) -> Id { self.peer }
    fn max_message_bytes(&self) -> usize { self.max_message_bytes }
    fn close(&self) { QueuedRpcAdapter::close(self); }
    fn send(&self, frame: Vec<u8>) -> Result<(), RpcError> {
        if frame.len() > self.max_message_bytes { return Err(RpcError::InvalidPayload); }
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        let queue = queue.as_mut().ok_or(RpcError::Disconnected)?;
        if queue.len() >= self.max_pending { return Err(RpcError::Overloaded); }
        queue.push_back(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_queue_is_bounded_and_closed_permanently() {
        let adapter = QueuedRpcAdapter::new(Id::new(), 256, 1).unwrap();
        assert_eq!(adapter.send(vec![0; 257]), Err(RpcError::InvalidPayload));
        adapter.send(vec![1]).unwrap();
        assert_eq!(adapter.send(vec![2]), Err(RpcError::Overloaded));
        assert_eq!(adapter.try_recv().unwrap(), Some(vec![1]));
        adapter.send(vec![2]).unwrap();
        adapter.close();
        assert_eq!(adapter.try_recv(), Err(RpcError::Disconnected));
        assert_eq!(adapter.send(vec![3]), Err(RpcError::Disconnected));
        assert!(QueuedRpcAdapter::new(Id::new(), 1, 1).is_err());
        assert!(QueuedRpcAdapter::new(Id::new(), 256, 0).is_err());
        assert!(QueuedRpcAdapter::new(Id::new(), 256, 65).is_err());
    }
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

impl RpcRequest {
    pub fn validate(&self) -> Result<(), RpcError> {
        if self.version != RPC_VERSION { return Err(RpcError::UnsupportedVersion); }
        if self.request_id == Id::nil() || self.payload.len() > RPC_MAX_PAYLOAD
            || self.operation.is_empty() || self.operation.len() > 256
            || self.timeout_ms == 0 || self.timeout_ms > RPC_MAX_TIMEOUT_MS {
            return Err(RpcError::InvalidPayload);
        }
        Ok(())
    }
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
    Unauthenticated,
    Forbidden,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcReply {
    pub version: u16,
    pub request_id: Id,
    pub result: Result<Vec<u8>, RpcError>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", deny_unknown_fields)]
pub enum PeerRpcFrame {
    Request(RpcRequest),
    Reply(RpcReply),
    Cancel { version: u16, request_id: Id },
}

impl PeerRpcFrame {
    pub fn encode(&self) -> Result<Vec<u8>, RpcError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| RpcError::InvalidPayload)?;
        if bytes.len() > MAX_FRAME_BYTES { return Err(RpcError::InvalidPayload); }
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RpcError> {
        if bytes.len() > MAX_FRAME_BYTES { return Err(RpcError::InvalidPayload); }
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let value = crate::schema::wire::unique_value(&mut decoder).map_err(|_| RpcError::InvalidPayload)?;
        decoder.end().map_err(|_| RpcError::InvalidPayload)?;
        let frame: Self = serde_json::from_value(value).map_err(|_| RpcError::InvalidPayload)?;
        frame.validate()?;
        Ok(frame)
    }

    fn validate(&self) -> Result<(), RpcError> {
        let (version, request_id) = match self {
            Self::Request(request) => {
                request.validate()?;
                (request.version, request.request_id)
            }
            Self::Reply(reply) => {
                if reply.result.as_ref().is_ok_and(|payload| payload.len() > RPC_MAX_PAYLOAD) { return Err(RpcError::InvalidReply); }
                (reply.version, reply.request_id)
            }
            Self::Cancel { version, request_id } => (*version, *request_id),
        };
        if version != RPC_VERSION { return Err(RpcError::UnsupportedVersion); }
        if request_id == Id::nil() { return Err(RpcError::InvalidPayload); }
        Ok(())
    }
}