<p align="center">
    <img src="splash.png" alt="Splash">
</p>
<div align="center">
    <a href="https://www.rust-lang.org"><img height=30em src="https://img.shields.io/badge/Rust-%2320232a?style=for-the-badge&logo=rust&logoColor=red&color=141414"></a>
    <a href="https://bevyengine.org"><img height=30em src="https://img.shields.io/badge/Bevy-%2320232a?style=for-the-badge&logo=bevy&logoColor=white&color=141414"></a>
</div>

# Flux

**⚠️ Still in early development. ⚠️**

A cohesive system for networking/replication, data binding and data storage using Bevy ECS.

Create complex configurations of entities (scenes, UI layouts, etc.) using a straightforward builder pattern.

# Instructions

To add to your project, simply run:

```
cargo add --git https://github.com/empathic-ai/flux.git
```

Within your app, you can use the builder like this:

```Rust
use bevy::prelude::*;
use flux::prelude::*;

fn main() {
  App::new()
    .add_startup_system(create_simple_ui)
    .run();
}

// Use the builder to create a simple sign up UI
fn create_simple_ui(mut commands: Commands) {
  commands.child().expand().v_list().small_padding().with_children(|parent| {
      parent.child().input_field("Username".to_string(), InputType::Default);
      parent.child().input_field("Email".to_string(), InputType::Default);
      parent.child().input_field("Password".to_string(), InputType::Password);
  });
}
```


## Logging

See the workspace [logging guide](../../docs/logging.md) for selectable groups,
local configuration, VS Code controls, and platform-specific behavior.

## Composable binding graphs

See [binding graphs](docs/binding-graphs.md) for typed multi-input functions,
two-way editing, scheduling, examples, and architectural references.

## Reactive collection views

See [reactive views](docs/reactive-views.md) for list/map rendering, nested
collections, row keys, and the role of `ReactiveView`.

## Lazy view construction

Use `lazy_children` / `lazy_entity_children` to construct content on first
display and retain it across hide/show cycles. Routes and visibility bindings
share the same lifecycle. See [lazy views](docs/lazy-views.md).

## Record access

See [record access](docs/record-access.md) for owner-only and server-only replication,
authenticated peer binding, and database authorization.

## Session lifecycle

Flux initializes `SessionState` independently of `DatabaseState`. Startup moves
the session from `Disconnected` to `Establishing`, then to `Ready` after obtaining
a valid peer ID and inserting the `Session` resource. Registration, HTTP, or
invalid peer-ID failures set `SessionState::Failed`; they do not mark the database
as failed. The server establishes its local session using the nil peer ID.

Session startup lives in `plugin/session.rs` and runs with the `futures` and
`tokio` features, independently of `surrealdb`. Clients without database support
can establish a session and relay network events. The `Session`, `is_session`,
and `register` prelude exports remain available.

With `surrealdb` enabled, a separate database startup system runs on
`OnEnter(SessionState::Ready)`, moving from `Disconnected` to `Connecting`.
This preserves session availability for existing database-ready consumers
without making session startup depend on database support. Database connection
and preparation failures use `DatabaseState::Failed`. Without `surrealdb`, the
database remains `Disconnected`.

ECS consumers can read `Res<State<SessionState>>`, react with
`OnEnter(SessionState::Failed)`, or gate session-dependent systems with
`run_if(in_state(SessionState::Ready))`. Systems requiring records should still
wait for `DatabaseState::Ready`.

`SessionState::Ready` means a peer session exists, not that a user has completed
OAuth login or that the server event transport remains connected. The existing
`NetworkState` enum is not currently driven by transport events. Session retry,
expiration, and ongoing transport connectivity are not tracked by this startup
lifecycle.

## Peer RPC transport

`PeerRpc` implements `RpcTransport` over a connection-specific byte sender. Both
ends may initiate calls using `RemoteServiceClient`; received requests dispatch
through the endpoint's `RpcServer`. Existing graph-backed service contracts use
the same schema adapters on this transport.

Construct one endpoint per connection with a transport-authenticated peer ID.
Pass that identity, not a claimed payload identity, to `receive`. Drive returned
request tasks concurrently while continuing to receive replies and cancellation
frames. Dropping a receive task drops its work. Call `disconnect` when the
connection closes and create a fresh endpoint on reconnect; never route frames
from an old connection into a new endpoint.

The bridge bounds outgoing exchanges to 64 and incoming tasks to 32 per endpoint.
Cancellation and disconnect apply even before an incoming task is first polled.
Frames reject duplicate JSON keys, trailing data, invalid IDs, unsupported
versions, and oversized payloads. Transport authentication, authorization,
fragmentation, timers, and connection lifecycle remain adapter responsibilities.

`flux::rpc_protocol::RpcConnectionAdapter` supplies a connection-bound
authenticated peer ID, encoded-message limit, frame sender, and close hook.
`QueuedRpcAdapter` supplies a bounded nonblocking outbound queue for transport
workers. Closing it discards unsent frames and permanently rejects new sends.
Use one adapter per connection. A local connection identifier does not establish
user authentication or permission to invoke an operation.

The `rpc` feature exposes `flux::rpc_peer::PeerRpc` and `PeerServiceRegistry`
without Bevy runtime services. Bind a dispatcher and adapter with `PeerRpc::bind`,
or use `with_adapter` with a concrete dispatcher. `RpcServer` implements the same
`RpcDispatcher` interface for Bevy service handlers. `PeerServiceRegistry`
registers typed request/response/error handlers with published contracts and a
runtime-provided deadline timer. Both dispatchers use the same service codec.
Handlers remain responsible for domain authorization.

Generated Bevy service clients continue to use `RpcTransport`; `PeerRpc`
implements that interface alongside request/response transports such as HTTP.
Service contracts do not select a transport: bind the client to the desired
transport and the server registry to an authenticated connection adapter.

Without the Bevy service runtime, `PeerRpc::call_versioned` takes an operation
name, typed request, published service contracts, timer, and timeout. It returns
an outer transport result containing the typed application result. Both ends
can initiate calls through this API. Dropping a pending call or reaching its
deadline sends cancellation; the peer connection bounds outstanding exchanges.
Replies are checked for matching identity and version, size, duplicate JSON
keys, trailing data, and the requested schema contracts. This API does not
automatically convert application event handlers into RPC services.

This API does not itself install production BLE or HTTP routing. BLE adapters
must enforce their smaller message limit, establish the appropriate trust model,
and drive dispatch tasks concurrently with receive processing. The peer engine
rejects oversized outbound requests before sending and substitutes a bounded
error for replies exceeding the connection's limit.

`flux::rpc_auth::RpcSessions` holds bounded, expiring authorization sessions
with distinct user, device, and service principals and exact operation grants.
Only trusted credential verification code may call `bind_verified`; receiving a
claimed identity is not verification. `SessionRpcDispatcher` checks the session
before dispatch and aborts active work on expiration, revocation, or replacement.
Connection IDs must be unique across connection lifetimes. Disconnect revokes
the connection session. Cancellation cannot roll back completed side effects.
Applications with an existing authoritative session store can implement
`RpcAuthorizer` and use `AuthorizedRpcDispatcher` instead of duplicating that store.
That lightweight wrapper checks dispatch permission but does not monitor an
external store for revocation while a handler is running.

The optional `rpc_secure` feature provides `RpcHandshake` and `SecureRpcAdapter`
using Snow's Noise XX implementation. Completed handshakes expose the remote
static key and handshake binding; application traffic requires approval against
an independently trusted key. Never approve a key solely because the handshake
reported it. The adapter encrypts over any `RpcConnectionAdapter`, accounts for
authentication-tag overhead, and closes on corrupted/replayed ciphertext or a
failed encrypted send. The underlying transport must preserve message order.
Workers must propagate receive/send failures into `PeerRpc::disconnect`.

Enrollment is not implemented by these primitives. Server-issued owner grants,
physical offline enrollment/recovery, identity-key persistence and rotation,
and binding verified grants to handshake identities remain application integration
work. The secure channel is opt-in and is not automatically enabled in existing
HTTP or BLE adapters. Host tests and ESP/web compile checks are not hardware or
mixed-release acceptance tests.

## Query expressions

See [query expressions](docs/query-expressions.md) for typed `query!` and
`query_one!` expressions, ECS and SurrealDB execution, handler system
parameters, structural ECS filters, and raw query escape hatches.
