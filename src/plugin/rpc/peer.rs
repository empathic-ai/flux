use crate::{rpc_protocol::*, types::Id};
use std::{collections::HashMap, sync::{Arc, Mutex}, future::Future, pin::Pin};

pub type PeerTask<T> = Pin<Box<dyn Future<Output = Result<T, RpcError>> + Send>>;

type Handler = Arc<dyn Fn(Id, Id, Vec<u8>) -> PeerTask<Vec<u8>> + Send + Sync>;
pub type PeerTimer = fn(std::time::Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;

#[derive(Clone)]
pub struct PeerServiceRegistry {
    handlers: HashMap<String, Handler>,
    timer: PeerTimer,
}

impl PeerServiceRegistry {
    pub fn new(timer: PeerTimer) -> Self {
        Self { handlers: HashMap::new(), timer }
    }

    pub fn register<Request, Response, Error, Contracts, HandlerFn, HandlerFuture>(
        &mut self, operation: &str, contracts: Contracts, handler: HandlerFn,
    ) -> anyhow::Result<()>
    where
        Request: crate::schema::Schema + serde::de::DeserializeOwned + Send + 'static,
        Response: crate::schema::Schema + serde::Serialize + Send + 'static,
        Error: crate::schema::Schema + serde::Serialize + Send + 'static,
        Contracts: crate::schema::wire::ServiceContractSet,
        HandlerFn: Fn(Id, Id, Request) -> HandlerFuture + Send + Sync + 'static,
        HandlerFuture: Future<Output = Result<Response, Error>> + Send + 'static,
    {
        contracts.validate_types::<Request, Response, Error>()?;
        let name = format!("{operation}{}", Contracts::OPERATION_SUFFIX);
        anyhow::ensure!(!operation.is_empty() && name.len() <= 256, "Invalid service operation");
        anyhow::ensure!(!self.handlers.contains_key(&name), "Duplicate service operation");
        let contracts = Arc::new(contracts);
        let handler = Arc::new(handler);
        self.handlers.insert(name, Arc::new(move |peer, request_id, payload| {
            let contracts = contracts.clone();
            let handler = handler.clone();
            Box::pin(async move {
                let (call, request) = decode_service_request::<Request, Contracts>(&contracts, &payload)?;
                let result = handler(peer, request_id, request).await;
                encode_service_reply::<Response, Error, Contracts>(&contracts, &call, result)
            })
        }));
        Ok(())
    }
}

impl RpcDispatcher for PeerServiceRegistry {
    fn dispatch(&self, authenticated_peer: Id, request: RpcRequest) -> RpcDispatch {
        let handler = self.handlers.get(&request.operation).cloned();
        let timer = self.timer;
        Box::pin(async move {
            let result = if let Err(error) = request.validate() {
                Err(error)
            } else if let Some(handler) = handler {
                match futures::future::select(handler(authenticated_peer, request.request_id, request.payload),
                    timer(std::time::Duration::from_millis(request.timeout_ms.into()))).await {
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

type Pending = HashMap<Id, Option<futures::channel::oneshot::Sender<Result<RpcReply, RpcError>>>>;
struct CallbackAdapter<SendFrame> {
    peer: Id,
    send: SendFrame,
}

impl<SendFrame> RpcConnectionAdapter for CallbackAdapter<SendFrame>
where SendFrame: Fn(Vec<u8>) -> Result<(), RpcError> + Send + Sync + 'static {
    fn authenticated_peer(&self) -> Id { self.peer }
    fn max_message_bytes(&self) -> usize { usize::MAX }
    fn send(&self, frame: Vec<u8>) -> Result<(), RpcError> { (self.send)(frame) }
}

struct Connection {
    connected: bool,
    pending: Pending,
    incoming: HashMap<Id, futures::future::AbortHandle>,
}

#[derive(Clone)]
pub struct PeerRpc {
    peer: Id,
    adapter: Arc<dyn RpcConnectionAdapter>,
    max_message_bytes: usize,
    server: Arc<dyn RpcDispatcher>,
    connection: Arc<Mutex<Connection>>,
}

struct PendingPermit {
    request_id: Id,
    connection: Arc<Mutex<Connection>>,
}

struct IncomingPermit {
    request_id: Id,
    connection: Arc<Mutex<Connection>>,
}

impl Drop for IncomingPermit {
    fn drop(&mut self) {
        self.connection.lock().unwrap_or_else(|error| error.into_inner()).incoming.remove(&self.request_id);
    }
}

impl Drop for PendingPermit {
    fn drop(&mut self) {
        self.connection.lock().unwrap_or_else(|error| error.into_inner()).pending.remove(&self.request_id);
    }
}

impl PeerRpc {
    pub fn new(authenticated_peer: Id, server: impl RpcDispatcher,
        send: impl Fn(Vec<u8>) -> Result<(), RpcError> + Send + Sync + 'static) -> Self {
        Self::with_adapter(server, Arc::new(CallbackAdapter { peer: authenticated_peer, send }))
    }

    pub fn with_adapter(server: impl RpcDispatcher, adapter: Arc<dyn RpcConnectionAdapter>) -> Self {
        Self::bind(Arc::new(server), adapter)
    }

    pub fn bind(server: Arc<dyn RpcDispatcher>, adapter: Arc<dyn RpcConnectionAdapter>) -> Self {
        Self { peer: adapter.authenticated_peer(), max_message_bytes: adapter.max_message_bytes(), server, adapter,
            connection: Arc::new(Mutex::new(Connection { connected: true, pending: HashMap::new(), incoming: HashMap::new() })) }
    }

    fn send_frame(&self, frame: PeerRpcFrame) -> Result<(), RpcError> {
        let bytes = frame.encode()?;
        if bytes.len() > self.max_message_bytes { return Err(RpcError::InvalidPayload); }
        self.adapter.send(bytes)
    }

    pub fn receive(&self, authenticated_peer: Id, bytes: &[u8]) -> Result<PeerTask<()>, RpcError> {
        if authenticated_peer != self.peer { return Err(RpcError::InvalidPayload); }
        if bytes.len() > self.max_message_bytes { return Err(RpcError::InvalidPayload); }
        let frame = PeerRpcFrame::decode(bytes)?;
        if !self.connection.lock().unwrap_or_else(|error| error.into_inner()).connected {
            return Err(RpcError::Disconnected);
        }
        let endpoint = self.clone();
        match frame {
            PeerRpcFrame::Reply(reply) => {
                let sender = endpoint.connection.lock().unwrap_or_else(|error| error.into_inner()).pending.get_mut(&reply.request_id).and_then(Option::take)
                    .ok_or(RpcError::InvalidReply)?;
                let _ = sender.send(Ok(reply));
                Ok(Box::pin(async { Ok(()) }))
            }
            PeerRpcFrame::Cancel { request_id, .. } => {
                if let Some(handle) = endpoint.connection.lock().unwrap_or_else(|error| error.into_inner()).incoming.get(&request_id) {
                    handle.abort();
                }
                Ok(Box::pin(async { Ok(()) }))
            }
            PeerRpcFrame::Request(request) => {
                let request_id = request.request_id;
                let (abort, registration) = futures::future::AbortHandle::new_pair();
                {
                    let mut connection = endpoint.connection.lock().unwrap_or_else(|error| error.into_inner());
                    if !connection.connected { return Err(RpcError::Disconnected); }
                    if connection.incoming.contains_key(&request_id) { return Err(RpcError::InvalidPayload); }
                    if connection.incoming.len() >= 32 { return Err(RpcError::Overloaded); }
                    connection.incoming.insert(request_id, abort);
                }
                let permit = IncomingPermit { request_id, connection: endpoint.connection.clone() };
                let prepared = endpoint.server.clone().prepare(authenticated_peer, request);
                Ok(Box::pin(async move {
                let _permit = permit;
                let result = futures::future::Abortable::new(
                    prepared, registration,
                ).await.unwrap_or(Err(RpcError::Cancelled));
                let reply = result.unwrap_or_else(|error| RpcReply { version: RPC_VERSION, request_id, result: Err(error) });
                if !endpoint.connection.lock().unwrap_or_else(|error| error.into_inner()).connected {
                    return Err(RpcError::Disconnected);
                }
                let frame = PeerRpcFrame::Reply(reply);
                let bytes = frame.encode();
                match bytes {
                    Ok(bytes) if bytes.len() <= endpoint.max_message_bytes => endpoint.adapter.send(bytes),
                    _ => endpoint.send_frame(PeerRpcFrame::Reply(RpcReply {
                        version: RPC_VERSION, request_id, result: Err(RpcError::InvalidReply),
                    })),
                }
                }))
            }
        }
    }

    pub fn disconnect(&self) {
        let pending = {
            let mut connection = self.connection.lock().unwrap_or_else(|error| error.into_inner());
            connection.connected = false;
            for handle in connection.incoming.values() { handle.abort(); }
            std::mem::take(&mut connection.pending)
        };
        for sender in pending.into_values().flatten() { let _ = sender.send(Err(RpcError::Disconnected)); }
        self.adapter.close();
        self.server.disconnect(self.peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Dispatcher(Arc<AtomicUsize>);

    #[test]
    fn disconnect_closes_adapter_and_discards_queued_frames() {
        let adapter = Arc::new(QueuedRpcAdapter::new(Id::new(), 256, 2).unwrap());
        let endpoint = PeerRpc::with_adapter(Dispatcher(Arc::new(AtomicUsize::new(0))), adapter.clone());
        adapter.send(vec![1]).unwrap();
        endpoint.disconnect();
        assert_eq!(adapter.try_recv(), Err(RpcError::Disconnected));
        assert_eq!(adapter.send(vec![2]), Err(RpcError::Disconnected));
    }

    impl RpcDispatcher for Dispatcher {
        fn dispatch(&self, _: Id, request: RpcRequest) -> RpcDispatch {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(RpcReply { version: RPC_VERSION, request_id: request.request_id, result: Ok(request.payload) })
            })
        }
    }

    #[test]
    fn standalone_dispatch_and_early_cancellation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let replies = Arc::new(Mutex::new(Vec::new()));
        let output = replies.clone();
        let peer = Id::new();
        let endpoint = PeerRpc::new(peer, Dispatcher(calls.clone()), move |bytes| {
            output.lock().unwrap().push(bytes);
            Ok(())
        });
        let request_id = Id::new();
        let request = PeerRpcFrame::Request(RpcRequest {
            version: RPC_VERSION, request_id, operation: "echo".into(), timeout_ms: 1000, payload: vec![17],
        }).encode().unwrap();
        let dispatch = endpoint.receive(peer, &request).unwrap();
        let cancel = PeerRpcFrame::Cancel { version: RPC_VERSION, request_id }.encode().unwrap();
        drop(endpoint.receive(peer, &cancel).unwrap());
        futures::executor::block_on(dispatch).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let cancelled = replies.lock().unwrap().pop().unwrap();
        assert!(matches!(PeerRpcFrame::decode(&cancelled).unwrap(), PeerRpcFrame::Reply(RpcReply { result: Err(RpcError::Cancelled), .. })));
        futures::executor::block_on(endpoint.receive(peer, &request).unwrap()).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let response = replies.lock().unwrap().pop().unwrap();
        match PeerRpcFrame::decode(&response).unwrap() {
            PeerRpcFrame::Reply(reply) => assert_eq!(reply.result, Ok(vec![17])),
            _ => panic!("expected reply"),
        }
    }
}

impl PeerRpc {
    pub fn exchange(&self, request: RpcRequest) -> PeerTask<RpcReply> {
        let endpoint = self.clone();
        Box::pin(async move {
            let request_id = request.request_id;
            let bytes = PeerRpcFrame::Request(request).encode()?;
            if bytes.len() > endpoint.max_message_bytes { return Err(RpcError::InvalidPayload); }
            let (sender, receiver) = futures::channel::oneshot::channel();
            {
                let mut connection = endpoint.connection.lock().unwrap_or_else(|error| error.into_inner());
                if !connection.connected { return Err(RpcError::Disconnected); }
                if connection.pending.contains_key(&request_id) { return Err(RpcError::InvalidPayload); }
                if connection.pending.len() >= 64 { return Err(RpcError::Overloaded); }
                connection.pending.insert(request_id, Some(sender));
            }
            let _permit = PendingPermit { request_id, connection: endpoint.connection.clone() };
            endpoint.adapter.send(bytes)?;
            receiver.await.map_err(|_| RpcError::Disconnected)?
        })
    }

    pub fn cancel(&self, request_id: Id) {
        let pending = {
            let mut connection = self.connection.lock().unwrap_or_else(|error| error.into_inner());
            if !connection.connected { return; }
            connection.pending.get_mut(&request_id).and_then(Option::take)
        };
        if let Some(sender) = pending { let _ = sender.send(Err(RpcError::Cancelled)); }
        let _ = self.send_frame(PeerRpcFrame::Cancel { version: RPC_VERSION, request_id });
    }

    pub fn call_versioned<Request, Response, Error, Contracts>(
        &self, operation: &str, request: Request, contracts: Contracts,
        timer: PeerTimer, timeout_ms: u32,
    ) -> PeerTask<Result<Response, Error>>
    where
        Request: crate::schema::Schema + serde::Serialize + Send + 'static,
        Response: crate::schema::Schema + serde::de::DeserializeOwned + Send + 'static,
        Error: crate::schema::Schema + serde::de::DeserializeOwned + Send + 'static,
        Contracts: crate::schema::wire::ServiceContractSet,
    {
        let endpoint = self.clone();
        let operation = operation.to_owned();
        Box::pin(async move {
            contracts.validate_types::<Request, Response, Error>().map_err(|_| RpcError::InvalidPayload)?;
            let call = contracts.call(serde_json::to_value(request).map_err(|_| RpcError::InvalidPayload)?)
                .map_err(|_| RpcError::InvalidPayload)?;
            let payload = serde_json::to_vec(&RpcContractCall { call: call.clone() })
                .map_err(|_| RpcError::InvalidPayload)?;
            let request_id = Id::new();
            let timeout_ms = timeout_ms.clamp(1, RPC_MAX_TIMEOUT_MS);
            let request = RpcRequest { version: RPC_VERSION, request_id,
                operation: format!("{operation}{}", Contracts::OPERATION_SUFFIX), timeout_ms, payload };
            request.validate()?;
            let mut guard = TypedCallGuard { endpoint: endpoint.clone(), request_id, armed: true };
            let reply = match futures::future::select(endpoint.exchange(request),
                timer(std::time::Duration::from_millis(timeout_ms.into()))).await {
                futures::future::Either::Left((reply, _)) => reply?,
                futures::future::Either::Right(_) => return Err(RpcError::DeadlineExceeded),
            };
            if reply.version != RPC_VERSION || reply.request_id != request_id { return Err(RpcError::InvalidReply); }
            guard.armed = false;
            let payload = reply.result?;
            if payload.len() > RPC_MAX_PAYLOAD { return Err(RpcError::InvalidReply); }
            let mut decoder = serde_json::Deserializer::from_slice(&payload);
            let value = crate::schema::wire::unique_value(&mut decoder).map_err(|_| RpcError::InvalidReply)?;
            decoder.end().map_err(|_| RpcError::InvalidReply)?;
            let result: Result<Contracts::Reply, Contracts::Reply> = serde_json::from_value(value)
                .map_err(|_| RpcError::InvalidReply)?;
            match result {
                Ok(reply) => {
                    let value = contracts.decode_response(&call, reply).map_err(|_| RpcError::InvalidReply)?;
                    Ok(Ok(serde_json::from_value(value).map_err(|_| RpcError::InvalidReply)?))
                }
                Err(reply) => {
                    let value = contracts.decode_error(&call, reply).map_err(|_| RpcError::InvalidReply)?;
                    Ok(Err(serde_json::from_value(value).map_err(|_| RpcError::InvalidReply)?))
                }
            }
        })
    }
}

struct TypedCallGuard {
    endpoint: PeerRpc,
    request_id: Id,
    armed: bool,
}

impl Drop for TypedCallGuard {
    fn drop(&mut self) {
        if self.armed { self.endpoint.cancel(self.request_id); }
    }
}