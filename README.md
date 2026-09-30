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

## Query expressions

See [query expressions](docs/query-expressions.md) for typed `query!` and
`query_one!` expressions, ECS and SurrealDB execution, handler system
parameters, structural ECS filters, and raw query escape hatches.
