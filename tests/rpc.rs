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