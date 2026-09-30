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