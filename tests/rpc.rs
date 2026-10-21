#![cfg(all(feature = "bevy_std", feature = "futures"))]

use bevy::prelude::*;
use flux::prelude::*;
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin, sync::{Arc, atomic::{AtomicUsize, Ordering}}, task::{Context, Poll}, time::Duration};

#[derive(Serialize, Deserialize)]
struct Echo(u32);
#[service]
trait EchoApi {
    #[wire("test.echo.v1")]
    async fn echo(request: Echo) -> Result<u32, String>;
}

fn never(_: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> { Box::pin(std::future::pending()) }

#[test]
fn child_authority_retires_independently_and_inherits_parent_revocation() {
    use flux::rpc_protocol::RpcAuthorityScope;
    let parent = RpcAuthorityScope::default();
    let first = parent.permit().child_scope();
    let second = parent.permit().child_scope();
    let first_permit = first.permit();
    first.retire();
    assert!(first_permit.check().is_err());
    assert!(parent.permit().check().is_ok());
    assert!(second.permit().check().is_ok());
    let second_permit = second.permit();
    parent.retire();
    assert!(second_permit.check().is_err());
    assert!(parent.permit().child_scope().permit().check().is_err());
}

#[cfg(feature = "rpc_secure")]
#[test]
fn secure_datagrams_tolerate_loss_without_replay_or_lane_confusion() {
    use flux::rpc_secure::{HandshakeRole, RpcHandshake};
    let mut initiator = RpcHandshake::new_datagrams(HandshakeRole::Initiator, &[7; 32], 1024).unwrap();
    let mut responder = RpcHandshake::new_datagrams(HandshakeRole::Responder, &[8; 32], 1024).unwrap();
    responder.read(&initiator.write().unwrap()).unwrap();
    initiator.read(&responder.write().unwrap()).unwrap();
    responder.read(&initiator.write().unwrap()).unwrap();
    let mut sender = initiator.approve_datagrams(&RpcHandshake::public_key(&[8; 32]).unwrap()).unwrap();
    let mut receiver = responder.approve_datagrams(&RpcHandshake::public_key(&[7; 32]).unwrap()).unwrap();
    let dropped = sender.encrypt(1, b"dropped").unwrap();
    let audio = sender.encrypt(1, b"audio").unwrap();
    let control = sender.encrypt(0, b"control").unwrap();
    assert!(sender.encrypt(2, b"invalid lane").is_err());
    assert!(sender.encrypt(0, b"").is_err());
    assert!(sender.encrypt(0, &[0; 1024]).is_err());
    assert!(receiver.decrypt(0, &[0; 24]).is_err());
    assert!(receiver.decrypt(0, &audio).is_err());
    let mut forged = audio.clone();
    forged[0] ^= 1;
    assert!(receiver.decrypt(1, &forged).is_err());
    assert_eq!(receiver.decrypt(1, &audio).unwrap(), b"audio");
    assert!(receiver.decrypt(1, &audio).is_err());
    assert!(receiver.decrypt(1, &dropped).is_err());
    assert_eq!(receiver.decrypt(0, &control).unwrap(), b"control");
    let reply = receiver.encrypt(0, b"reply").unwrap();
    assert_eq!(sender.decrypt(0, &reply).unwrap(), b"reply");
    receiver.close();
    assert!(receiver.decrypt(0, &sender.encrypt(0, b"closed").unwrap()).is_err());
}

#[cfg(feature = "rpc_secure")]
#[test]
fn secure_datagrams_reject_ordered_handshake_mode() {
    use flux::rpc_secure::{HandshakeRole, RpcHandshake};
    let mut initiator = RpcHandshake::new_datagrams(HandshakeRole::Initiator, &[7; 32], 1024).unwrap();
    let mut responder = RpcHandshake::new(HandshakeRole::Responder, &[8; 32], 1024).unwrap();
    responder.read(&initiator.write().unwrap()).unwrap();
    assert!(initiator.read(&responder.write().unwrap()).is_err());
    assert!(RpcHandshake::new(HandshakeRole::Initiator, &[7; 32], 1024).unwrap()
        .approve_datagrams(&[8; 32]).is_err());
    assert!(RpcHandshake::new_datagrams(HandshakeRole::Initiator, &[7; 32], 1024).unwrap()
        .approve(&[8; 32]).is_err());
}

#[derive(Reflect, Reactive, Clone, Serialize, Deserialize, flux::schema::Schema)]
#[schema(wire, name = "test.nested_rpc")]
struct NestedRpc { detail: RpcDetail }

#[derive(Reflect, Reactive, Clone, Serialize, Deserialize, flux::schema::Schema)]
struct RpcDetail { total: u32 }

fn nested_catalogs() -> (flux::schema::Catalog, flux::schema::Catalog) {
    use flux::schema::*;
    let mut detail = RpcDetail::describe();
    detail.fields[0].name = "count".into();
    let mut roots = std::collections::BTreeMap::from([
        ("test.nested_rpc".into(), NestedRpc::describe()), ("RpcDetail".into(), detail),
    ]);
    let baseline = Catalog::init(&roots).unwrap();
    let detail = roots.get_mut("RpcDetail").unwrap();
    detail.fields[0].name = "total".into();
    detail.fields[0].rename_from = Some("count".into());
    let mut draft = baseline.draft("nested_rpc_rename".into(), &roots).unwrap();
    let adapter = &mut draft.updates.get_mut("RpcDetail").unwrap().migration;
    adapter.downgrade = Some(Adapter {
        from_fingerprint: adapter.upgrade.to_fingerprint.clone(), to_fingerprint: adapter.upgrade.from_fingerprint.clone(),
        missing: Default::default(), allow_drop: Default::default(), transforms: Default::default(),
    });
    let current = baseline.finalize(&draft).unwrap();
    (baseline, current)
}

fn nested_event_catalog() -> anyhow::Result<flux::schema::Catalog> {
    static CATALOGS: std::sync::OnceLock<(flux::schema::Catalog, flux::schema::Catalog)> = std::sync::OnceLock::new();
    Ok(CATALOGS.get_or_init(nested_catalogs).1.clone())
}

fn nested_contracts() -> flux::schema::wire::GraphServiceContracts {
    flux::schema::wire::GraphServiceContracts::new::<NestedRpc, NestedRpc, NestedRpc>(nested_event_catalog().unwrap()).unwrap()
}

#[test]
fn generated_client_uses_standalone_service_dispatcher() {
    use flux::rpc_peer::PeerServiceRegistry;
    use flux::rpc_protocol::{RpcDispatch, RpcDispatcher};
    struct StandaloneTransport(PeerServiceRegistry, Id);
    impl RpcTransport for StandaloneTransport {
        fn exchange(&self, request: RpcRequest) -> TaskResult<RpcReply, RpcError> {
            let task: RpcDispatch = self.0.dispatch(self.1, request);
            TaskResult::new(task)
        }
        fn cancel(&self, _: Id) {}
    }
    let peer = Id::new();
    let mut registry = PeerServiceRegistry::new(never);
    registry.register::<NestedRpc, NestedRpc, NestedRpc, _, _, _>(NestedRpc::OPERATION,
        nested_contracts(), move |authenticated_peer, request_id, request| async move {
            assert_eq!(authenticated_peer, peer);
            assert_ne!(request_id, Id::nil());
            if request.detail.total == 0 { Err(request) } else { Ok(request) }
        }).unwrap();
    let client = NestedApiRemoteClient::new(RemoteServiceClient::new(
        Arc::new(StandaloneTransport(registry, peer)), never, 1000));
    let mut success = Box::pin(client.nested(NestedRpc { detail: RpcDetail { total: 7 } }));
    assert!(matches!(poll(success.as_mut()), Poll::Ready(Ok(NestedRpc { detail: RpcDetail { total: 7 } }))));
    let mut failure = Box::pin(client.nested(NestedRpc { detail: RpcDetail { total: 0 } }));
    assert!(matches!(poll(failure.as_mut()), Poll::Ready(Err(CallError::Application(NestedRpc { detail: RpcDetail { total: 0 } })))));
}

flux::register_managed_event!(NestedRpc, nested_event_catalog);

#[service]
trait NestedApi {
    #[wire("test.nested.v1")]
    #[contracts(nested_contracts)]
    async fn nested(request: NestedRpc) -> Result<NestedRpc, NestedRpc>;
}

#[test]
fn nested_events_and_rpc_share_graph_adaptation() {
    use flux::schema::*;
    use flux::schema::wire::{GraphCall, GraphServiceContracts, ServiceContractSet};
    let current = nested_event_catalog().unwrap();
    let mut baseline = current.clone();
    let history = &mut baseline.roots.get_mut("RpcDetail").unwrap().history;
    history.contracts.truncate(1);
    history.migrations.clear();
    let graph = baseline.graph_ref("test.nested_rpc").unwrap();
    let payload = GraphValue::new(&baseline, "test.nested_rpc", serde_json::json!({"detail":{"count":7}}), ValueMode::Complete).unwrap();
    let mut event_bytes = b"FLUX-GRAPH-1\0".to_vec();
    event_bytes.extend(serde_json::to_vec(&serde_json::json!({"peer_id": Id::nil(), "payload": payload})).unwrap());
    let decoded = deserialize_network_event(&event_bytes).unwrap();
    assert_eq!(decoded.get_ev::<NestedRpc>().unwrap().detail.total, 7);

    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut app = App::new();
    app.register_service(NestedApi::handlers(move |_, context, request: NestedRpc| {
        observed.fetch_add(1, Ordering::SeqCst);
        async move {
            assert_ne!(context.peer_id, Id::nil());
            if request.detail.total == 0 { Err(request) } else { Ok(request) }
        }
    }));
    let server = RpcServer::new(app.world().resource::<WireServiceRegistry>().clone(),
        app.world().resource::<ServiceRegistry>().clone(), app.world().resource::<Executor>().clone(), never);
    let peer = Id::new();
    let client = NestedApiRemoteClient::new(RemoteServiceClient::new(Arc::new(Loopback { server: server.clone(), peer }), never, 1000));
    let mut latest = Box::pin(client.nested(NestedRpc { detail: RpcDetail { total: 9 } }));
    assert!(matches!(poll(latest.as_mut()), Poll::Ready(Ok(NestedRpc { detail: RpcDetail { total: 9 } }))));

    let call = GraphCall { format_version: FORMAT_VERSION, request: payload, response: graph.clone(), error: graph.clone() };
    let exchange = |call: &GraphCall| {
        let mut task = Box::pin(server.dispatch(peer, RpcRequest { version: RPC_VERSION, request_id: Id::new(),
            operation: format!("{}{}", NestedRpc::OPERATION, GraphServiceContracts::OPERATION_SUFFIX), timeout_ms: 1000,
            payload: serde_json::to_vec(&serde_json::json!({"call": call})).unwrap() }));
        match poll(task.as_mut()) { Poll::Ready(Ok(reply)) => reply, _ => panic!("RPC did not complete") }
    };
    let reply: Result<GraphValue, GraphValue> = serde_json::from_slice(&exchange(&call).result.unwrap()).unwrap();
    let reply = reply.unwrap();
    assert_eq!(reply.graph, graph);
    assert_eq!(reply.decode(&baseline, &graph, ValueMode::Complete).unwrap(), serde_json::json!({"detail":{"count":7}}));
    let mut failure = call.clone();
    failure.request.value["detail"]["count"] = 0.into();
    let reply: Result<GraphValue, GraphValue> = serde_json::from_slice(&exchange(&failure).result.unwrap()).unwrap();
    assert_eq!(reply.unwrap_err().decode(&baseline, &graph, ValueMode::Complete).unwrap(), serde_json::json!({"detail":{"count":0}}));
    let before = calls.load(Ordering::SeqCst);
    let mut forged = call.clone();
    forged.response.contracts.values_mut().next().unwrap().fingerprint = "forged".into();
    assert_eq!(exchange(&forged).result, Err(RpcError::InvalidPayload));
    assert_eq!(calls.load(Ordering::SeqCst), before);
    let mut unsupported = current;
    unsupported.roots.get_mut("RpcDetail").unwrap().history.migrations[0].downgrade = None;
    let contracts = GraphServiceContracts::new::<NestedRpc, NestedRpc, NestedRpc>(unsupported).unwrap();
    assert!(contracts.decode_request(&call).is_err());
}

fn expired(_: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> { Box::pin(std::future::ready(())) }
fn poll<T>(task: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
    task.poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
}

fn server(pending: bool) -> (App, RpcServer) {
    let mut app = App::new();
    app.register_service(EchoApi::handlers(move |_executor, context, request: Echo| async move {
        assert_ne!(context.peer_id, Id::nil());
        if pending { std::future::pending::<()>().await; }
        Ok(request.0)
    }));
    let wire = app.world().resource::<WireServiceRegistry>().clone();
    let server = RpcServer::new(wire, app.world().resource::<ServiceRegistry>().clone(),
        app.world().resource::<Executor>().clone(), never);
    (app, server)
}

struct Loopback { server: RpcServer, peer: Id }
#[test]
fn versioned_peer_service_uses_shared_authorization_sessions() {
    use flux::{rpc_auth::{RpcPrincipal, RpcSessions, SessionRpcDispatcher}, rpc_peer::PeerServiceRegistry,
        rpc_protocol::QueuedRpcAdapter, schema::wire::ServiceContractSet};
    let peer = Id::new();
    let sessions = RpcSessions::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut registry = PeerServiceRegistry::new(never);
    registry.register::<NestedRpc, NestedRpc, NestedRpc, _, _, _>(NestedRpc::OPERATION, nested_contracts(),
        move |_, _, request| {
            observed.fetch_add(1, Ordering::SeqCst);
            async move { Ok(request) }
        }).unwrap();
    let dispatcher = SessionRpcDispatcher::new(Arc::new(registry), sessions.clone(), never);
    let adapter = Arc::new(QueuedRpcAdapter::new(peer, 16 * 1024, 4).unwrap());
    let endpoint = PeerRpc::with_adapter(dispatcher, adapter.clone());
    let call = nested_contracts().call(serde_json::json!({"detail":{"total":7}})).unwrap();
    let operation = format!("{}.graph", NestedRpc::OPERATION);
    let receive = || {
        let bytes = PeerRpcFrame::Request(RpcRequest {
            version: RPC_VERSION, request_id: Id::new(), operation: operation.clone(), timeout_ms: 1000,
            payload: serde_json::to_vec(&flux::rpc_protocol::RpcContractCall { call: call.clone() }).unwrap(),
        }).encode().unwrap();
        let mut task = endpoint.receive(peer, &bytes).unwrap();
        assert!(matches!(poll(Pin::new(&mut task)), Poll::Ready(Ok(()))));
        match PeerRpcFrame::decode(&adapter.try_recv().unwrap().unwrap()).unwrap() {
            PeerRpcFrame::Reply(reply) => reply.result,
            _ => panic!("expected service reply"),
        }
    };
    assert_eq!(receive(), Err(RpcError::Unauthenticated));
    sessions.bind_verified(peer, RpcPrincipal::Device(Id::new()), vec!["other.graph".into()], Duration::from_secs(60)).unwrap();
    assert_eq!(receive(), Err(RpcError::Forbidden));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    sessions.bind_verified(peer, RpcPrincipal::Device(Id::new()), vec![operation.clone()], Duration::from_secs(60)).unwrap();
    assert!(receive().is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    endpoint.disconnect();
    assert_eq!(sessions.principal(peer), Err(RpcError::Unauthenticated));
    assert_eq!(adapter.try_recv(), Err(RpcError::Disconnected));
}

#[test]
fn peer_rpc_uses_connection_adapter_limits_and_identity() {
    struct TestAdapter {
        peer: Id,
        sent: std::sync::Mutex<Vec<Vec<u8>>>,
    }
    impl RpcConnectionAdapter for TestAdapter {
        fn authenticated_peer(&self) -> Id { self.peer }
        fn max_message_bytes(&self) -> usize { 256 }
        fn send(&self, bytes: Vec<u8>) -> Result<(), RpcError> {
            assert!(bytes.len() <= self.max_message_bytes());
            self.sent.lock().unwrap().push(bytes);
            Ok(())
        }
    }
    let (_app, server) = server(false);
    let adapter = Arc::new(TestAdapter { peer: Id::new(), sent: Default::default() });
    let endpoint = PeerRpc::with_adapter(server, adapter.clone());
    let request = RpcRequest {
        version: RPC_VERSION, request_id: Id::new(), operation: Echo::OPERATION.into(),
        timeout_ms: 1000, payload: serde_json::to_vec(&Echo(17)).unwrap(),
    };
    let mut oversized = request.clone();
    oversized.payload = vec![255; 256];
    let bytes = PeerRpcFrame::Request(oversized.clone()).encode().unwrap();
    assert!(matches!(endpoint.receive(adapter.peer, &bytes), Err(RpcError::InvalidPayload)));
    let mut call = Box::pin(endpoint.exchange(oversized));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Err(RpcError::InvalidPayload))));
    assert!(adapter.sent.lock().unwrap().is_empty());
    let bytes = PeerRpcFrame::Request(request).encode().unwrap();
    assert!(matches!(endpoint.receive(Id::new(), &bytes), Err(RpcError::InvalidPayload)));
    let mut dispatch = Box::pin(endpoint.receive(adapter.peer, &bytes).unwrap());
    assert!(matches!(poll(dispatch.as_mut()), Poll::Ready(Ok(()))));
    assert_eq!(adapter.sent.lock().unwrap().len(), 1);
}

#[test]
fn peer_rpc_reserves_request_id_until_exchange_finishes() {
    let (_app, server) = server(false);
    let peer = Id::new();
    let endpoint = PeerRpc::new(peer, server, |_| Ok(()));
    let request = RpcRequest {
        version: RPC_VERSION, request_id: Id::new(), operation: Echo::OPERATION.into(),
        timeout_ms: 1000, payload: serde_json::to_vec(&Echo(17)).unwrap(),
    };
    let mut first = Box::pin(endpoint.exchange(request.clone()));
    assert!(poll(first.as_mut()).is_pending());
    let reply = PeerRpcFrame::Reply(RpcReply {
        version: RPC_VERSION, request_id: request.request_id, result: Ok(vec![]),
    }).encode().unwrap();
    drop(endpoint.receive(peer, &reply).unwrap());
    let mut duplicate = Box::pin(endpoint.exchange(request.clone()));
    assert!(matches!(poll(duplicate.as_mut()), Poll::Ready(Err(RpcError::InvalidPayload))));
    assert!(matches!(poll(first.as_mut()), Poll::Ready(Ok(_))));
    let mut next = Box::pin(endpoint.exchange(request.clone()));
    assert!(poll(next.as_mut()).is_pending());
    drop(first);
    drop(endpoint.receive(peer, &reply).unwrap());
    assert!(matches!(poll(next.as_mut()), Poll::Ready(Ok(_))));
}

#[test]
fn peer_rpc_cancellation_before_poll_and_frame_validation() {
    let (_app, server) = server(false);
    let peer = Id::new();
    let replies = Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = replies.clone();
    let endpoint = PeerRpc::new(peer, server, move |bytes| {
        output.lock().unwrap().push(bytes);
        Ok(())
    });
    let request_id = Id::new();
    let request = PeerRpcFrame::Request(RpcRequest {
        version: RPC_VERSION, request_id, operation: Echo::OPERATION.into(),
        timeout_ms: 1000, payload: serde_json::to_vec(&Echo(17)).unwrap(),
    }).encode().unwrap();
    let mut dispatch = Box::pin(endpoint.receive(peer, &request).unwrap());
    assert!(matches!(endpoint.receive(peer, &request), Err(RpcError::InvalidPayload)));
    let cancel = PeerRpcFrame::Cancel { version: RPC_VERSION, request_id }.encode().unwrap();
    drop(endpoint.receive(peer, &cancel).unwrap());
    assert!(matches!(poll(dispatch.as_mut()), Poll::Ready(Ok(()))));
    let bytes = replies.lock().unwrap().pop().unwrap();
    match PeerRpcFrame::decode(&bytes).unwrap() {
        PeerRpcFrame::Reply(reply) => assert_eq!(reply.result, Err(RpcError::Cancelled)),
        _ => panic!("expected cancellation reply"),
    }
    let mut dispatch = Box::pin(endpoint.receive(peer, &request).unwrap());
    endpoint.disconnect();
    assert!(matches!(poll(dispatch.as_mut()), Poll::Ready(Err(RpcError::Disconnected))));
    assert!(replies.lock().unwrap().is_empty());
    assert!(matches!(endpoint.receive(peer, &request), Err(RpcError::Disconnected)));
    assert!(PeerRpcFrame::decode(br#"{"kind":"Cancel","kind":"Cancel","message":{}}"#).is_err());
    let mut trailing = cancel.clone();
    trailing.extend_from_slice(b" {}");
    assert!(PeerRpcFrame::decode(&trailing).is_err());
    assert!(PeerRpcFrame::Cancel { version: RPC_VERSION + 1, request_id }.encode().is_err());
    assert!(PeerRpcFrame::Cancel { version: RPC_VERSION, request_id: Id::nil() }.encode().is_err());
    assert!(PeerRpcFrame::Reply(RpcReply {
        version: RPC_VERSION, request_id, result: Ok(vec![0; RPC_MAX_PAYLOAD + 1]),
    }).encode().is_err());
}

#[test]
fn peer_rpc_routes_both_directions_and_disconnects_pending_calls() {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    let (_left_app, left_server) = server(false);
    let (_right_app, right_server) = server(false);
    let left_id = Id::new();
    let right_id = Id::new();
    let to_left = Arc::new(Mutex::new(VecDeque::new()));
    let to_right = Arc::new(Mutex::new(VecDeque::new()));
    let outgoing = to_right.clone();
    let left = PeerRpc::new(right_id, left_server, move |bytes| {
        outgoing.lock().unwrap().push_back(bytes);
        Ok(())
    });
    let outgoing = to_left.clone();
    let right = PeerRpc::new(left_id, right_server, move |bytes| {
        outgoing.lock().unwrap().push_back(bytes);
        Ok(())
    });
    let left_client = EchoApiRemoteClient::new(RemoteServiceClient::new(Arc::new(left.clone()), never, 1000));
    let right_client = EchoApiRemoteClient::new(RemoteServiceClient::new(Arc::new(right.clone()), never, 1000));
    let mut left_call = Box::pin(left_client.echo(Echo(11)));
    let mut right_call = Box::pin(right_client.echo(Echo(22)));
    assert!(poll(left_call.as_mut()).is_pending());
    assert!(poll(right_call.as_mut()).is_pending());
    let request = to_right.lock().unwrap().pop_front().unwrap();
    let mut dispatch = Box::pin(right.receive(left_id, &request).unwrap());
    assert!(matches!(poll(dispatch.as_mut()), Poll::Ready(Ok(()))));
    let request = to_left.lock().unwrap().pop_front().unwrap();
    let mut dispatch = Box::pin(left.receive(right_id, &request).unwrap());
    assert!(matches!(poll(dispatch.as_mut()), Poll::Ready(Ok(()))));
    let reply = to_left.lock().unwrap().pop_front().unwrap();
    assert!(matches!(left.receive(Id::new(), &reply), Err(RpcError::InvalidPayload)));
    let mut delivery = Box::pin(left.receive(right_id, &reply).unwrap());
    assert!(matches!(poll(delivery.as_mut()), Poll::Ready(Ok(()))));
    assert!(matches!(left.receive(right_id, &reply), Err(RpcError::InvalidReply)));
    let reply = to_right.lock().unwrap().pop_front().unwrap();
    let mut delivery = Box::pin(right.receive(left_id, &reply).unwrap());
    assert!(matches!(poll(delivery.as_mut()), Poll::Ready(Ok(()))));
    assert!(matches!(poll(left_call.as_mut()), Poll::Ready(Ok(11))));
    assert!(matches!(poll(right_call.as_mut()), Poll::Ready(Ok(22))));
    let mut pending = Box::pin(left_client.echo(Echo(33)));
    assert!(poll(pending.as_mut()).is_pending());
    left.disconnect();
    assert!(matches!(poll(pending.as_mut()), Poll::Ready(Err(CallError::Remote(RpcError::Disconnected)))));
    let mut later = Box::pin(left_client.echo(Echo(44)));
    assert!(matches!(poll(later.as_mut()), Poll::Ready(Err(CallError::Remote(RpcError::Disconnected)))));
}

impl RpcTransport for Loopback {
    fn exchange(&self, request: RpcRequest) -> TaskResult<RpcReply, RpcError> {
        self.server.dispatch(self.peer, request)
    }
    fn cancel(&self, request_id: Id) { self.server.cancel(self.peer, request_id); }
}

fn request(request_id: Id) -> RpcRequest {
    RpcRequest { version: RPC_VERSION, request_id, operation: Echo::OPERATION.into(),
        timeout_ms: 1_000, payload: serde_json::to_vec(&Echo(7)).unwrap() }
}

#[test]
fn scoped_authority_wakes_all_suspended_requests() {
    use flux::rpc_protocol::{AuthorizedRpcDispatcher, RpcAuthorityScope, RpcAuthorizer,
        RpcDispatcher, RpcDispatch, ScopedRpcAuthorizer};
    struct Policy;
    impl RpcAuthorizer for Policy {
        fn authorize(&self, _: Id, _: &str) -> Result<(), RpcError> { Ok(()) }
    }
    struct Pending;
    impl RpcDispatcher for Pending {
        fn dispatch(&self, _: Id, _: RpcRequest) -> RpcDispatch {
            Box::pin(std::future::pending())
        }
    }
    struct WakeCounter(AtomicUsize);
    impl std::task::Wake for WakeCounter {
        fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
        fn wake_by_ref(self: &Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
    }
    let scope = RpcAuthorityScope::default();
    let permit = scope.permit();
    let dispatcher = AuthorizedRpcDispatcher::new(Arc::new(Pending),
        Arc::new(ScopedRpcAuthorizer::new(permit.clone(), Arc::new(Policy))));
    let mut requests: Vec<_> = (0..3).map(|_| {
        let counter = Arc::new(WakeCounter(AtomicUsize::new(0)));
        let waker = std::task::Waker::from(counter.clone());
        let mut task = dispatcher.dispatch(Id::new(), request(Id::new()));
        assert!(task.as_mut().poll(&mut Context::from_waker(&waker)).is_pending());
        (task, counter, waker)
    }).collect();
    requests.pop();
    scope.retire();
    for (mut task, counter, waker) in requests {
        assert_eq!(counter.0.load(Ordering::SeqCst), 1);
        assert!(matches!(task.as_mut().poll(&mut Context::from_waker(&waker)),
            Poll::Ready(Ok(RpcReply { result: Err(RpcError::Unauthenticated), .. }))));
    }
    let mut late = Box::pin(permit.revoked());
    assert!(poll(late.as_mut()).is_ready());
}

#[test]
fn scoped_authority_rejects_queued_requests_without_encryption() {
    use flux::rpc_protocol::{AuthorizedRpcDispatcher, RpcAuthorityScope, RpcAuthorizer,
        RpcDispatcher, RpcDispatch, ScopedRpcAuthorizer};
    struct Policy(Id);
    impl RpcAuthorizer for Policy {
        fn authorize(&self, peer: Id, operation: &str) -> Result<(), RpcError> {
            if peer == self.0 && operation == Echo::OPERATION { Ok(()) }
            else { Err(RpcError::Forbidden) }
        }
    }
    struct Dispatch(Arc<AtomicUsize>);
    impl RpcDispatcher for Dispatch {
        fn dispatch(&self, _: Id, request: RpcRequest) -> RpcDispatch {
            let calls = self.0.clone();
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(RpcReply { version: RPC_VERSION, request_id: request.request_id, result: Ok(vec![]) })
            })
        }
    }
    let peer = Id::new();
    let scope = RpcAuthorityScope::default();
    let permit = scope.permit();
    let policy = Arc::new(ScopedRpcAuthorizer::new(permit.clone(), Arc::new(Policy(peer))));
    assert_eq!(policy.authorize(Id::new(), Echo::OPERATION), Err(RpcError::Forbidden));
    let calls = Arc::new(AtomicUsize::new(0));
    let dispatcher = AuthorizedRpcDispatcher::new(Arc::new(Dispatch(calls.clone())), policy);
    let mut successful = Box::pin(dispatcher.dispatch(peer, request(Id::new())));
    assert!(matches!(poll(successful.as_mut()), Poll::Ready(Ok(RpcReply { result: Ok(_), .. }))));
    let mut queued = Box::pin(dispatcher.dispatch(peer, request(Id::new())));
    scope.retire();
    assert!(matches!(poll(queued.as_mut()), Poll::Ready(Ok(RpcReply { result: Err(RpcError::Unauthenticated), .. }))));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(permit.check(), Err(RpcError::Unauthenticated));
    let replacement = RpcAuthorityScope::default();
    let current = replacement.permit();
    assert!(current.check().is_ok());
    assert!(permit.check().is_err());
    drop(replacement);
    assert!(current.check().is_err());
}

#[test]
fn scoped_authority_rechecks_pending_work_and_completed_replies() {
    use flux::rpc_protocol::{AuthorizedRpcDispatcher, RpcAuthorityScope, RpcAuthorizer,
        RpcDispatcher, RpcDispatch, ScopedRpcAuthorizer};
    struct Policy;
    impl RpcAuthorizer for Policy {
        fn authorize(&self, _: Id, _: &str) -> Result<(), RpcError> { Ok(()) }
    }
    struct Dispatch {
        scope: Arc<RpcAuthorityScope>,
        polls: Arc<AtomicUsize>,
    }
    impl RpcDispatcher for Dispatch {
        fn dispatch(&self, _: Id, request: RpcRequest) -> RpcDispatch {
            let scope = self.scope.clone();
            let polls = self.polls.clone();
            Box::pin(std::future::poll_fn(move |_| {
                if polls.fetch_add(1, Ordering::SeqCst) == 0 {
                    Poll::Pending
                } else {
                    scope.retire();
                    Poll::Ready(Ok(RpcReply {
                        version: RPC_VERSION, request_id: request.request_id, result: Ok(vec![1]),
                    }))
                }
            }))
        }
    }
    for retire_while_pending in [true, false] {
        let scope = Arc::new(RpcAuthorityScope::default());
        let polls = Arc::new(AtomicUsize::new(0));
        let dispatcher = AuthorizedRpcDispatcher::new(
            Arc::new(Dispatch { scope: scope.clone(), polls: polls.clone() }),
            Arc::new(ScopedRpcAuthorizer::new(scope.permit(), Arc::new(Policy))),
        );
        let mut task = Box::pin(dispatcher.dispatch(Id::new(), request(Id::new())));
        assert!(poll(task.as_mut()).is_pending());
        if retire_while_pending { scope.retire(); }
        assert!(matches!(poll(task.as_mut()), Poll::Ready(Ok(RpcReply {
            result: Err(RpcError::Unauthenticated), ..
        }))));
        assert_eq!(polls.load(Ordering::SeqCst), if retire_while_pending { 1 } else { 2 });
    }
}

#[test]
fn wire_roundtrip_preserves_typed_response() {
    let (_app, server) = server(false);
    let client = RemoteServiceClient::new(Arc::new(Loopback { server, peer: Id::new() }), never, 1_000);
    let client = EchoApiRemoteClient::new(client);
    let mut call = Box::pin(client.echo(Echo(42)));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Ok(42))));
}

#[test]
fn cancellation_is_scoped_to_authenticated_peer() {
    let (_app, server) = server(true);
    let peer = Id::new();
    let request_id = Id::new();
    let mut call = Box::pin(server.dispatch(peer, request(request_id)));
    assert!(poll(call.as_mut()).is_pending());
    server.cancel(Id::new(), request_id);
    assert!(poll(call.as_mut()).is_pending());
    server.cancel(peer, request_id);
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Err(RpcError::Cancelled))));
    let mut retry = Box::pin(server.dispatch(peer, request(request_id)));
    assert!(poll(retry.as_mut()).is_pending());
    server.disconnect(peer);
    assert!(matches!(poll(retry.as_mut()), Poll::Ready(Err(RpcError::Cancelled))));
}

#[test]
fn active_duplicates_and_per_peer_overload_are_rejected() {
    let (_app, server) = server(true);
    let peer = Id::new();
    let first = Id::new();
    let mut calls = Vec::new();
    for index in 0..32 {
        let mut call = Box::pin(server.dispatch(peer, request(if index == 0 { first } else { Id::new() })));
        assert!(poll(call.as_mut()).is_pending());
        calls.push(call);
    }
    let mut duplicate = Box::pin(server.dispatch(peer, request(first)));
    assert!(matches!(poll(duplicate.as_mut()), Poll::Ready(Err(RpcError::InvalidPayload))));
    let mut overload = Box::pin(server.dispatch(peer, request(Id::new())));
    assert!(matches!(poll(overload.as_mut()), Poll::Ready(Err(RpcError::Overloaded))));
    drop(calls);
    let mut next = Box::pin(server.dispatch(peer, request(first)));
    assert!(poll(next.as_mut()).is_pending());
}

struct BrokenTransport { wrong_reply: bool, cancellations: Arc<AtomicUsize> }
impl RpcTransport for BrokenTransport {
    fn exchange(&self, _request: RpcRequest) -> TaskResult<RpcReply, RpcError> {
        let wrong = self.wrong_reply;
        TaskResult::new(async move {
            if !wrong { std::future::pending::<()>().await; }
            Ok(RpcReply { version: RPC_VERSION, request_id: Id::new(), result: Ok(b"{\"Ok\":7}".to_vec()) })
        })
    }
    fn cancel(&self, _request_id: Id) { self.cancellations.fetch_add(1, Ordering::SeqCst); }
}

#[test]
fn deadlines_and_dropped_callers_cancel_transport_work() {
    let cancellations = Arc::new(AtomicUsize::new(0));
    let transport = Arc::new(BrokenTransport { wrong_reply: false, cancellations: cancellations.clone() });
    let client = RemoteServiceClient::new(transport.clone(), expired, 1);
    let mut call = Box::pin(client.call(Echo(1)));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Err(CallError::Remote(RpcError::DeadlineExceeded)))));
    assert_eq!(cancellations.load(Ordering::SeqCst), 1);
    let client = RemoteServiceClient::new(transport, never, 1_000);
    let mut call = Box::pin(client.call(Echo(1)));
    assert!(poll(call.as_mut()).is_pending());
    drop(call);
    assert_eq!(cancellations.load(Ordering::SeqCst), 2);
}

#[test]
fn mismatched_reply_is_rejected() {
    let client = RemoteServiceClient::new(Arc::new(BrokenTransport {
        wrong_reply: true, cancellations: Arc::new(AtomicUsize::new(0)),
    }), never, 1_000);
    let mut call = Box::pin(client.call(Echo(1)));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Err(CallError::Remote(RpcError::InvalidReply)))));
}

#[derive(Serialize, Deserialize, flux::schema::Schema)]
struct OldMessage { name: String }

#[derive(Serialize, Deserialize, flux::schema::Schema)]
struct CurrentMessage {
    #[schema(rename_from = "name")]
    display_name: String,
}

#[derive(Serialize, Deserialize, flux::schema::Schema)]
struct OldFailure { message: String }

#[derive(Serialize, Deserialize, flux::schema::Schema)]
struct CurrentFailure {
    #[schema(rename_from = "message")]
    reason: String,
}

#[service]
trait HistoricalApi {
    #[wire("test.historical")]
    async fn echo(request: CurrentMessage) -> Result<CurrentMessage, CurrentFailure>;
}

impl ServiceRequest for OldMessage {
    type Response = OldMessage;
    type Error = OldFailure;
}

impl WireRequest for OldMessage {
    const OPERATION: &'static str = "test.historical";
}

fn renamed_history<Old: flux::schema::Schema, Current: flux::schema::Schema>() -> (flux::schema::History, flux::schema::History) {
    use flux::schema::{Adapter, History, Migration};
    let baseline = History::new(Old::describe().reconcile(None).unwrap()).unwrap();
    let target = Current::describe().reconcile(baseline.contracts.last()).unwrap();
    let upgrade = Adapter { from_fingerprint: baseline.contracts[0].fingerprint().unwrap(), to_fingerprint: target.fingerprint().unwrap(), missing: Default::default(), allow_drop: Default::default(), transforms: Default::default() };
    let downgrade = Adapter { from_fingerprint: upgrade.to_fingerprint.clone(), to_fingerprint: upgrade.from_fingerprint.clone(), missing: Default::default(), allow_drop: Default::default(), transforms: Default::default() };
    let mut current = baseline.clone();
    current.append(target, Migration { id: "rename".into(), from_revision: 0, upgrade, downgrade: Some(downgrade) }).unwrap();
    (baseline, current)
}

#[derive(Serialize, Deserialize, flux::schema::Schema)]
struct ManagedRequest { fail: bool }

#[derive(Debug, Serialize, Deserialize, flux::schema::Schema)]
struct ManagedReply { accepted: bool }

#[derive(Debug, Serialize, Deserialize, flux::schema::Schema)]
#[serde(tag = "kind")]
enum ManagedError { Unavailable }

fn managed_contracts() -> flux::schema::wire::ServiceContracts {
    use flux::schema::{History, Schema, wire::ServiceContracts};
    static CONTRACTS: std::sync::OnceLock<ServiceContracts> = std::sync::OnceLock::new();
    CONTRACTS.get_or_init(|| ServiceContracts {
        request: History::new(ManagedRequest::describe().reconcile(None).unwrap()).unwrap(),
        response: History::new(ManagedReply::describe().reconcile(None).unwrap()).unwrap(),
        error: History::new(ManagedError::describe().reconcile(None).unwrap()).unwrap(),
    }).clone()
}

#[service]
trait ManagedApi {
    #[wire("test.managed")]
    #[contracts(managed_contracts)]
    async fn call(request: ManagedRequest) -> Result<ManagedReply, ManagedError>;
}

#[test]
fn generated_client_and_registration_use_published_contracts() {
    let mut app = App::new();
    app.register_service(ManagedApi::handlers(|_, _, request: ManagedRequest| async move {
        if request.fail { Err(ManagedError::Unavailable) } else { Ok(ManagedReply { accepted: true }) }
    }));
    let server = RpcServer::new(app.world().resource::<WireServiceRegistry>().clone(),
        app.world().resource::<ServiceRegistry>().clone(), app.world().resource::<Executor>().clone(), never);
    let endpoint = RemoteServiceClient::new(Arc::new(Loopback { server, peer: Id::new() }), never, 1_000);
    let client = ManagedApiRemoteClient::new(endpoint.clone());
    let mut call = Box::pin(client.call(ManagedRequest { fail: false }));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Ok(ManagedReply { accepted: true }))));
    let mut call = Box::pin(client.call(ManagedRequest { fail: true }));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Err(CallError::Application(ManagedError::Unavailable)))));
    let mut unversioned = Box::pin(endpoint.call(ManagedRequest { fail: false }));
    assert!(matches!(poll(unversioned.as_mut()), Poll::Ready(Err(CallError::Remote(RpcError::UnknownOperation)))));
}

#[test]
fn versioned_rpc_adapts_requests_responses_and_errors_before_typed_dispatch() {
    use flux::schema::wire::ServiceContracts;
    let (old_request, request) = renamed_history::<OldMessage, CurrentMessage>();
    let (old_response, response) = renamed_history::<OldMessage, CurrentMessage>();
    let (old_error, error) = renamed_history::<OldFailure, CurrentFailure>();
    let old = ServiceContracts { request: old_request, response: old_response, error: old_error };
    let current = ServiceContracts { request, response, error };
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut app = App::new();
    app.register_service(HistoricalApi::handlers(move |_executor, context, request: CurrentMessage| {
        assert_ne!(context.peer_id, Id::nil());
        observed.fetch_add(1, Ordering::SeqCst);
        async move {
            if request.display_name == "fail" { return Err(CurrentFailure { reason: "rejected".into() }); }
            Ok(request)
        }
    }));
    app.world_mut().resource_mut::<WireServiceRegistry>().expose_versioned::<CurrentMessage>(current).unwrap();
    let server = RpcServer::new(app.world().resource::<WireServiceRegistry>().clone(),
        app.world().resource::<ServiceRegistry>().clone(), app.world().resource::<Executor>().clone(), never);
    let client = RemoteServiceClient::new(Arc::new(Loopback { server, peer: Id::new() }), never, 1_000);
    let mut call = Box::pin(client.call_versioned(OldMessage { name: "historical".into() }, old.clone()));
    let Poll::Ready(Ok(reply)) = poll(call.as_mut()) else { panic!("Historical reply failed") };
    assert_eq!(reply.name, "historical");
    let mut call = Box::pin(client.call_versioned(OldMessage { name: "fail".into() }, old.clone()));
    let Poll::Ready(Err(CallError::Application(error))) = poll(call.as_mut()) else { panic!("Historical error failed") };
    assert_eq!(error.message, "rejected");
    let mut forged = old;
    forged.request.contracts[0].subject = "6b6259ad-4a9f-43b3-9ec3-9e9207f673b6".into();
    let mut call = Box::pin(client.call_versioned(OldMessage { name: "forged".into() }, forged));
    assert!(matches!(poll(call.as_mut()), Poll::Ready(Err(CallError::Remote(RpcError::InvalidPayload)))));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}