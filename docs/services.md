# Typed services and workflows

The `bevy_std` + `futures` API provides `Executor`, `TaskResult`, and
`ServiceAppExt`, with generated local and remote clients. `RpcTransport` supplies
the adapter-specific request/reply exchange; notifications and audio remain separate.

## Generated declarations

```rust,ignore
#[service]
trait SessionApi {
    #[wire("example.session.login.v1")]
    async fn login(request: LoginRequest) -> Result<LoginResponse, LoginError>;
    #[system]
    async fn logout(request: LogoutRequest) -> Result<LogoutResponse, LogoutError>;
}

app.register_service(SessionApi::handlers(login_handler, logout_handler));
let client = SessionApiClient::new(registry.clone(), executor.clone(), peer_id);
let response = client.login(request).await?;
```

The declaration expands into a marker, a typed client, request-associated response
and error implementations, and a handler registration bundle. Pass handlers in
declaration order. It does not produce a Rust trait to implement: the handler
bundle permits each system to declare different Bevy system parameters.

Async implementations take `(Executor, RequestContext, Request)`. Methods marked
`#[system]` take `In<(RequestContext, Request)>` and compatible Bevy parameters,
returning `TaskResult<Response, Error>` from a synchronous function. The async
declaration describes the caller's experience, not a system borrowing ECS across
an await. Each request type must be unique across generated method declarations.
Service declarations currently exclude generic methods, receivers, defaults, and
supertraits. Methods with `#[wire("stable.operation.v1")]` also appear on the
generated `SessionApiRemoteClient` and register their codecs in `WireServiceRegistry`.
Wire requests, responses, and application errors must support Serde. Methods without
the annotation remain local-only. Construct registry snapshots after registration.

`app.register_service(service)` invokes the service's `Service::register`
implementation. Register individual async functions with
`register_handler::<Request, _, _>`. Each `ServiceRequest` declares its
`Response` and application `Error`. Async handlers take
`(Executor, RequestContext, Request)` in that order.

`register_system_handler::<Request, _, _>` accepts an ordinary Bevy system with
`In<(RequestContext, Request)>` and arbitrary compatible system parameters.
It returns `TaskResult<Response, Error>`. The dispatcher runs the system on the
world, releases its borrows, and then awaits the returned task. Application
errors for this registration must implement `From<ExecuteError>`.

```rust,ignore
fn handler(
    In((context, request)): In<(RequestContext, MyRequest)>,
    executor: Res<Executor>,
) -> TaskResult<MyResponse> {
    executor.run_system(first_step, request)
        .and_then(|value| async move { external_operation(value).await })
}
```

Tasks implement `Future`, so both `.await` and `.and_then(...)` work. Task
closures and results own their data; ECS resource and query borrows cannot
cross an await. A task returning an application `Result` from a system has
both execution and application errors; handle these explicitly.

The executor is lazy: polling queues a system invocation. Its bounded queue
holds 256 jobs and processes at most 64 per `Last` schedule. A full queue returns
`Overloaded`; teardown returns `Closed` and discards unstarted jobs. Dropping a
pending task skips its unstarted system mutation. It does not undo completed
steps or external side effects.

Systems currently run once per invocation. `Local` state and change detection
are not retained across calls. Use resources for persistent application state;
do not use this API as a substitute for scheduled event-reader systems.

`RequestContext` is supplied by trusted local code. Remote adapters must derive
the peer identity from their authenticated connection, never accept a peer identity
asserted by a remote payload. Registry dispatch uses local Rust type identity;
wire dispatch uses explicit operation names and protocol version 1.

## Remote Calls

Construct `RemoteServiceClient` with an `Arc<dyn RpcTransport>`, a functioning
`RpcTimer`, and a timeout in milliseconds, then pass it to the generated remote
client's `new` constructor. Transport errors are `CallError::Remote`; decoded
application errors are `CallError::Application`.

Payloads are limited to 64 KiB, deadlines to 30 seconds, and shared remote clients
to 64 in-flight calls. `RpcServer` admits 256 active calls overall and 32 per peer.
Replies must match the request ID and protocol version. Adapters must independently
bound encoded bodies before deserializing them and authenticate the reply endpoint.

Dropping a caller or reaching its deadline invokes transport cancellation. The
server supports cancellation by authenticated peer and request ID, plus disconnect
cleanup. Cancellation is best-effort: an early cancel can arrive before dispatch,
and completed side effects cannot be rolled back. Server deadlines bound remaining
work. Active duplicate IDs are rejected, but completed IDs are not remembered.
There are no automatic retries, replay protection, or exactly-once guarantees.

Empathic's browser HTTP adapter uses `POST /rpc` and `DELETE /rpc/{request_id}`,
session credentials, origin checks, and bounded encoded bodies. It exposes
`HttpServices.session.session_info(GetSessionInfo {}).await` and registers the
read-only `empathic.session.info.v1` operation on the server. Fetch and timer futures
are bridged from the browser-local runtime and aborted when dropped. Existing
event/SSE and audio routes are unchanged. Native/BLE/ESP RPC integration and live
browser-session validation are not covered by this implementation.