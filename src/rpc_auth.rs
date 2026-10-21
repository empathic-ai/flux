use crate::{rpc_peer::PeerTimer, rpc_protocol::*, types::Id};
use futures::future::{AbortHandle, Abortable};
use std::{collections::{HashMap, HashSet}, sync::{Arc, Mutex}, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcPrincipal {
    User(Id),
    Device(Id),
    Service(Id),
}

struct Session {
    generation: Id,
    principal: RpcPrincipal,
    expires_at: Instant,
    operations: HashSet<String>,
    active: HashMap<Id, AbortHandle>,
}

impl Drop for Session {
    fn drop(&mut self) {
        for handle in self.active.values() { handle.abort(); }
    }
}

#[derive(Clone, Default)]
pub struct RpcSessions {
    sessions: Arc<Mutex<HashMap<Id, Session>>>,
}

impl RpcSessions {
    pub fn bind_verified(&self, connection: Id, principal: RpcPrincipal, operations: Vec<String>, lifetime: Duration)
        -> Result<Id, RpcError> {
        let subject = match principal { RpcPrincipal::User(subject) | RpcPrincipal::Device(subject) | RpcPrincipal::Service(subject) => subject };
        if connection == Id::nil() || subject == Id::nil() || lifetime.is_zero()
            || lifetime > Duration::from_secs(86_400) || operations.len() > 64
            || operations.iter().any(|operation| operation.is_empty() || operation.len() > 256) {
            return Err(RpcError::InvalidPayload);
        }
        let now = Instant::now();
        let mut sessions = self.sessions.lock().unwrap_or_else(|error| error.into_inner());
        sessions.retain(|_, session| session.expires_at > now);
        if !sessions.contains_key(&connection) && sessions.len() >= 256 { return Err(RpcError::Overloaded); }
        let generation = Id::new();
        sessions.insert(connection, Session {
            generation, principal, expires_at: now + lifetime, operations: operations.into_iter().collect(), active: HashMap::new(),
        });
        Ok(generation)
    }

    pub fn revoke(&self, connection: Id) {
        self.sessions.lock().unwrap_or_else(|error| error.into_inner()).remove(&connection);
    }

    pub fn principal(&self, connection: Id) -> Result<RpcPrincipal, RpcError> {
        let mut sessions = self.sessions.lock().unwrap_or_else(|error| error.into_inner());
        if sessions.get(&connection).is_some_and(|session| session.expires_at <= Instant::now()) {
            sessions.remove(&connection);
        }
        sessions.get(&connection).map(|session| session.principal).ok_or(RpcError::Unauthenticated)
    }
}

impl RpcAuthorizer for RpcSessions {
    fn authorize(&self, connection: Id, operation: &str) -> Result<(), RpcError> {
        let sessions = self.sessions.lock().unwrap_or_else(|error| error.into_inner());
        let session = sessions.get(&connection).filter(|session| session.expires_at > Instant::now())
            .ok_or(RpcError::Unauthenticated)?;
        if !session.operations.contains(operation) { return Err(RpcError::Forbidden); }
        Ok(())
    }
}

struct ActivePermit {
    sessions: RpcSessions,
    connection: Id,
    generation: Id,
    request_id: Id,
}

impl Drop for ActivePermit {
    fn drop(&mut self) {
        let mut sessions = self.sessions.sessions.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(session) = sessions.get_mut(&self.connection).filter(|session| session.generation == self.generation) {
            session.active.remove(&self.request_id);
        }
    }
}

pub struct SessionRpcDispatcher {
    dispatcher: Arc<dyn RpcDispatcher>,
    sessions: RpcSessions,
    timer: PeerTimer,
}

impl SessionRpcDispatcher {
    pub fn new(dispatcher: Arc<dyn RpcDispatcher>, sessions: RpcSessions, timer: PeerTimer) -> Self {
        Self { dispatcher, sessions, timer }
    }
}

impl RpcDispatcher for SessionRpcDispatcher {
    fn prepare(self: Arc<Self>, connection: Id, request: RpcRequest) -> RpcDispatch {
        self.dispatch(connection, request)
    }
    fn disconnect(&self, connection: Id) {
        self.sessions.revoke(connection);
        self.dispatcher.disconnect(connection);
    }

    fn dispatch(&self, connection: Id, request: RpcRequest) -> RpcDispatch {
        let sessions = self.sessions.clone();
        let dispatcher = self.dispatcher.clone();
        let timer = self.timer;
        let accepted_generation = sessions.sessions.lock().unwrap_or_else(|error| error.into_inner())
            .get(&connection).filter(|session| session.expires_at > Instant::now())
            .map(|session| session.generation);
        Box::pin(async move {
            let request_id = request.request_id;
            let work = async {
                request.validate()?;
                let (abort, registration) = AbortHandle::new_pair();
                let (generation, lifetime) = {
                    let mut all = sessions.sessions.lock().unwrap_or_else(|error| error.into_inner());
                    let total: usize = all.values().map(|session| session.active.len()).sum();
                    let session = all.get_mut(&connection).filter(|session| session.expires_at > Instant::now())
                        .ok_or(RpcError::Unauthenticated)?;
                    if accepted_generation != Some(session.generation) { return Err(RpcError::Unauthenticated); }
                    if !session.operations.contains(&request.operation) { return Err(RpcError::Forbidden); }
                    if session.active.contains_key(&request_id) { return Err(RpcError::InvalidPayload); }
                    if total >= 256 || session.active.len() >= 32 { return Err(RpcError::Overloaded); }
                    session.active.insert(request_id, abort);
                    (session.generation, session.expires_at.saturating_duration_since(Instant::now()))
                };
                let _permit = ActivePermit { sessions: sessions.clone(), connection, generation, request_id };
                let call = Box::pin(Abortable::new(async { dispatcher.dispatch(connection, request).await }, registration));
                match futures::future::select(call, timer(lifetime)).await {
                    futures::future::Either::Left((result, _)) => result.map_err(|_| RpcError::Unauthenticated)?,
                    futures::future::Either::Right(_) => Err(RpcError::Unauthenticated),
                }
            };
            Ok(work.await.unwrap_or_else(|error| RpcReply { version: RPC_VERSION, request_id, result: Err(error) }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, pin::Pin, sync::atomic::{AtomicUsize, Ordering}, task::{Context, Poll}};

    fn never(_: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> { Box::pin(std::future::pending()) }
    fn expired(_: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> { Box::pin(std::future::ready(())) }
    fn poll(task: &mut RpcDispatch) -> Poll<Result<RpcReply, RpcError>> {
        task.as_mut().poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
    }

    struct Handler(Arc<AtomicUsize>);
    impl RpcDispatcher for Handler {
        fn dispatch(&self, _: Id, _: RpcRequest) -> RpcDispatch {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(std::future::pending())
        }
    }

    fn request() -> RpcRequest {
        RpcRequest { version: RPC_VERSION, request_id: Id::new(), operation: "read.graph".into(), timeout_ms: 1000, payload: vec![] }
    }

    fn assert_denied(mut task: RpcDispatch, expected: RpcError) {
        match poll(&mut task) {
            Poll::Ready(Ok(reply)) => assert_eq!(reply.result, Err(expected)),
            _ => panic!("authorization should reject immediately"),
        }
    }

    #[test]
    fn authorization_blocks_unverified_forbidden_and_expired_sessions() {
        let sessions = RpcSessions::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let dispatcher = SessionRpcDispatcher::new(Arc::new(Handler(calls.clone())), sessions.clone(), never);
        let connection = Id::new();
        assert_denied(dispatcher.dispatch(connection, request()), RpcError::Unauthenticated);
        sessions.bind_verified(connection, RpcPrincipal::Device(Id::new()), vec!["write.graph".into()], Duration::from_secs(60)).unwrap();
        assert_denied(dispatcher.dispatch(connection, request()), RpcError::Forbidden);
        sessions.sessions.lock().unwrap().get_mut(&connection).unwrap().expires_at = Instant::now();
        assert_denied(dispatcher.dispatch(connection, request()), RpcError::Unauthenticated);
        assert_eq!(sessions.principal(connection), Err(RpcError::Unauthenticated));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn revocation_and_replacement_cancel_only_the_old_session() {
        let sessions = RpcSessions::default();
        let dispatcher = SessionRpcDispatcher::new(Arc::new(Handler(Arc::new(AtomicUsize::new(0)))), sessions.clone(), never);
        let connection = Id::new();
        let principal = RpcPrincipal::User(Id::new());
        sessions.bind_verified(connection, principal, vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
        let request = request();
        let mut first = dispatcher.dispatch(connection, request.clone());
        assert!(poll(&mut first).is_pending());
        assert_denied(dispatcher.dispatch(connection, request.clone()), RpcError::InvalidPayload);
        sessions.bind_verified(connection, principal, vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
        let mut second = dispatcher.dispatch(connection, request.clone());
        assert!(poll(&mut second).is_pending());
        assert_denied(first, RpcError::Unauthenticated);
        assert_denied(dispatcher.dispatch(connection, request), RpcError::InvalidPayload);
        sessions.revoke(connection);
        assert_denied(second, RpcError::Unauthenticated);
    }

    #[test]
    fn session_deadline_drops_inflight_work() {
        let sessions = RpcSessions::default();
        let connection = Id::new();
        sessions.bind_verified(connection, RpcPrincipal::Service(Id::new()), vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
        let dispatcher = SessionRpcDispatcher::new(Arc::new(Handler(Arc::new(AtomicUsize::new(0)))), sessions.clone(), expired);
        assert_denied(dispatcher.dispatch(connection, request()), RpcError::Unauthenticated);
        assert!(sessions.sessions.lock().unwrap().get(&connection).unwrap().active.is_empty());
    }

    #[test]
    fn delayed_requests_cannot_inherit_a_new_session() {
        let sessions = RpcSessions::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let dispatcher = SessionRpcDispatcher::new(Arc::new(Handler(calls.clone())), sessions.clone(), never);
        let connection = Id::new();
        let before_login = dispatcher.dispatch(connection, request());
        sessions.bind_verified(connection, RpcPrincipal::User(Id::new()), vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
        assert_denied(before_login, RpcError::Unauthenticated);
        let before_replacement = dispatcher.dispatch(connection, request());
        sessions.bind_verified(connection, RpcPrincipal::User(Id::new()), vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
        assert_denied(before_replacement, RpcError::Unauthenticated);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn queued_peer_requests_cannot_inherit_replacement_authority() {
        let sessions = RpcSessions::default();
        let connection = Id::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let adapter = Arc::new(QueuedRpcAdapter::new(connection, 4096, 4).unwrap());
        let dispatcher = SessionRpcDispatcher::new(Arc::new(Handler(calls.clone())), sessions.clone(), never);
        let endpoint = crate::rpc_peer::PeerRpc::with_adapter(dispatcher, adapter.clone());
        for initially_authenticated in [false, true] {
            if initially_authenticated {
                sessions.bind_verified(connection, RpcPrincipal::User(Id::new()), vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
            }
            let frame = PeerRpcFrame::Request(request()).encode().unwrap();
            let pending = endpoint.receive(connection, &frame).unwrap();
            sessions.bind_verified(connection, RpcPrincipal::User(Id::new()), vec!["read.graph".into()], Duration::from_secs(60)).unwrap();
            futures::executor::block_on(pending).unwrap();
            let reply = PeerRpcFrame::decode(&adapter.try_recv().unwrap().unwrap()).unwrap();
            assert!(matches!(reply, PeerRpcFrame::Reply(RpcReply { result: Err(RpcError::Unauthenticated), .. })));
            sessions.revoke(connection);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}