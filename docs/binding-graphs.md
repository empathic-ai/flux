# Composable ECS binding graphs

Flux's `binding::graph` module provides the shared reactive dataflow runtime.
A path is a source or projection in the graph, rather than the entire binding.
Multiple sources can feed a function, one result can feed multiple functions,
and a function result can be followed by another path (including an `Id` jump).
Every binding builder creates a graph; there is no descriptor-based runtime.

## Architectural basis

The design draws on several established systems, with different responsibilities:

- [Bevy system composition](https://docs.rs/bevy/latest/bevy/ecs/system/struct.PipeSystem.html)
  supplies typed function inputs and ECS parameter injection. The implementation
  uses the workspace's pinned Bevy 0.16 fork, not the latest API. Like
  `EntityFunc`, functions retain their initialized system state; unlike an
  action callback, a graph computation produces a value before destinations run.
- [Unreal Blueprint functions](https://dev.epicgames.com/documentation/en-us/unreal-engine/functions-in-unreal-engine)
  distinguish computations from actions. Here, graph computations must implement
  `ReadOnlySystem`, and installation rejects deferred writes and non-Send system
  parameters. Bevy 0.16 permits `Commands` in a `ReadOnlySystem`, so that marker
  alone does not enforce this boundary. Deferred flags must be checked *after*
  system initialization. Rust cannot prevent I/O or interior mutation hidden
  inside user closures: computations must also honor this contract themselves.
- [RxJS combineLatest](https://rxjs.dev/api/index/function/combineLatest)
  motivates combining the current values of several inputs. These graphs carry
  state snapshots, not event streams. Typed `process` callbacks combine them
  using ordinary application code.
- [Angular computed signals](https://angular.dev/guide/signals)
  demonstrate derived state and dynamic dependency replacement. This runtime
  deliberately evaluates every node each update rather than claiming precise
  invalidation for arbitrary Bevy queries. Component insertions/removals,
  resource changes, and changes in `DBConfig` mappings therefore refresh values.
- [Bidirectional lenses](https://www.cis.upenn.edu/~bcpierce/papers/lenses-full.pdf)
  motivate explicit write-back semantics. A filter or union cannot infer where
  to insert an edited item or which original duplicate to change. Direct links
  and explicit conversions support two-way updates; arbitrary graph edges do
  not acquire an automatic inverse.

## Editing UI values

For display-only widgets, live write-back, or manual Save/Cancel, use the
[collection and form editing adapters](editable-list-design.md). They preserve
active drafts and item identity; a raw one-way graph sink does not enforce a
read-only UI or protect a locally edited destination.

## Checked paths and builder integration

### Typed values and one-shot writes

`path!` preserves the final value type through fields, literal indices,
Option traversal, and entity jumps. `IntoBindingPath::Value` and
`IntoBindingExpr::Value` expose that type to generic APIs. Path Results retain it,
so `?` is optional at a builder or command call.

```rust,ignore
// Model.number: i32
let path: TypedBindingPath<i32> = path!(entity, Model.number)?;
commands.set_property(path, 42);
commands.set_property(path!(entity, Model.number), 43);
// Compile error: the source carries i32, not String.
// commands.set_property(path!(entity, Model.number), "wrong".to_owned());

let label: BindingExpr<String> = process(
    path!(entity, Model.number), |number: i32| Ok(number.to_string()),
);
```

Import `flux::prelude::*` for `FluxCommandsExt`. Writes run once when Bevy applies
Commands, using the existing reflected writer and current entity/Id mappings.
The component must be registered as `Reactive`, and `DBConfig` must be available,
just as for other Flux bindings. Missing records, None traversal, invalid indices,
and incompatible dynamic data remain runtime errors, routed through Bevy’s command
error handler. The command does not install a persistent binding.

`process` and `process_system` return `BindingExpr<Output>` and require typed
inputs to match callback arguments, including nested expressions and tuples.

For `process`, typed inputs infer positional closure parameter types, so
`|available, configured|` needs no annotations. The direct `Fn` bound uses
nightly's `unboxed_closures` and `tuple_trait` features alongside Flux's existing
nightly features; the custom `ProcessFn` adapter alone does not guide closure
parameter inference. Erased inputs still need argument types supplied by the
caller. A `collect()` result may separately need `Vec<_>` to choose its collection
type, even when all closure parameters and element types are known.

`set_property` accepts a typed **path**: computations have an output type but no
unique writable source. Use the editing APIs for explicit inverse operations.

Runtime APIs remain available: `BindingPath::new` creates a string-based path,
and `path.erase()` / `expression.erase()` explicitly discard static information.
Explicit old `BindingPath` or `BindingExpr` annotations may need `.erase()` or a
typed annotation. Custom implementations of the conversion traits now declare
`type Value`; use `Untyped` for dynamic inputs. Graph nodes, component-path builder
targets, and graph sinks still use runtime reflection checks. Typed paths are
accepted by graph construction and editing helpers and erased at that boundary.

`as Type` remains a declaration of the expected dynamic shape, not proof of the
stored value’s type. It determines the static leaf type while runtime reflection
still validates the actual data.

### Inline binding expressions

Use `process` for an ordinary function and `process_system` for a Bevy system.
Both return a single-use `BindingExpr<Output>`, which owns a recipe rather than a node
in an existing graph. Builders compile and install it automatically:

```rust,ignore
parent.child().bind_list_from(
    process(
        (
            path!(device_view_entity, DeviceView.available_networks),
            path!(device_view_entity, ReactiveView.value as Id -> Device.wifi_configs),
        ),
        |available: HashSet<Uuid>, configured: HashMap<Uuid, WifiConfig>| {
            let mut ids: Vec<_> = available.into_iter()
                .filter(|id| !configured.contains_key(id)).collect();
            ids.sort_unstable();
            Ok(ids)
        },
    ),
    render_network,
);

let adjusted = process_system(
    path!(source, Model.number),
    |In((value,)): In<(i32,)>, settings: Res<Settings>| Ok(value + settings.offset),
);
let label = process(adjusted, |value: i32| Ok(value.to_string()));
parent.child().bind_from(label, component_path!(TextLabel.text));
```

Inputs may be paths, macro `Result<TypedBindingPath<T>>` values, expressions, or
`Result<BindingExpr<T>>` values. Pass one input directly, a heterogeneous tuple of
1–16 inputs, or `()` for no inputs. Callbacks use typed positional arguments
and return `anyhow::Result<Output>`; systems use `In<(A, B, ...)>`.
Missing values, conversion failures, ECS dependency validation, and read-only
system restrictions are the same as the explicit graph APIs.

`bind_from` and `bind_list_from` route construction and installation failures
to Bevy's command error handler. Use `try_bind_from(...)?` or
`try_bind_list_from(...)?` to handle construction failures immediately, before
any binding/list commands are queued. Installation errors still occur at command
application, and evaluation errors remain available through `BindingGraphs::errors`.

For shared nodes, multiple sinks, projections, or two-way links, keep using an
explicit graph. `graph.add(expression)?` consumes an expression and returns a
normal `BindingNode`; a failed add rolls back newly constructed nodes.
Expressions are not cloneable and do not implicitly merge existing graphs:
reuse a returned node within its graph to share a computation and its system state.
`bind_node(graph, node, target)` and `bind_list_node(graph, node, render)` remain
available unchanged.

All bindings use `BindingGraphPlugin` and evaluate in PostUpdate. Direct paths
use source-change delivery; computations maintain their calculated result.
Wrapping a path in an identity `process` deliberately selects maintained delivery;
it is not equivalent for locally editable destinations.

The macros are implemented in `flux-derive` and re-exported from
`flux::prelude::*`. They check Rust types and fields without constructing or
reading any ECS values:

```rust,ignore
graph.two_way(
    path!(source, Model.number)?,
    path!(target, Model.number)?,
    BindingConflict::Reject,
);
```

`path!(entity, Component.field)` returns `BindingResult<TypedBindingPath<T>>`,
where `T` is the final field’s Rust type.
`component_path!(Component.field)` returns `BindingResult<ComponentBindingPath>`
without an entity, for builder destinations. `property_path!(Type.field)` returns
the property string alone and can still be used with the older string APIs.
Root and jump component names use their reflected type identity, including when
the Rust type was imported under an alias.

The checked builders accept either a resolved path or the macro's Result, so
ordinary UI methods returning `&mut Self` need neither `?` nor `unwrap()`:

```rust,ignore
parent.child().bind_from(
    path!(wifi_config_view_entity, WifiConfigView.is_editing),
    component_path!(Control.is_visible),
);
parent.child().bind_list_from(
    path!(app_entity, AppView.devices),
    |In(entity), mut commands: Commands| {
        commands.entity(entity).builder().reactive_view(Id::nil());
    },
);
```

Path errors from these builders use Bevy's deferred command error handler.
For fallible application setup, use the `try_bind_from` and
`try_bind_list_from` methods. The original signatures of `bind_component_property`,
`bind_component`, `bind_component_to`, `bind_self_property`, and `bind_list`
remain available as graph-building convenience methods.

**One representation, one runtime.** Every builder prepares and installs a
`BindingGraph`. Direct path sinks use `BindingWritePolicy::OnSourceChange`.
An explicit `graph.bind(...)` or expression
uses `Maintain`. The former preserves local edits until the source changes;
the latter maintains the calculated output. Explicit graphs may opt into
source-change delivery with `bind_with_policy` on a direct source node.
There is no `PreparedBinding` enum, `Binding` component, `BindingsConfig`,
`into_bindings()`, or `FluxWorld::update()` compatibility layer.
Inspect installed graphs with `BindingGraphs::iter()` and their actual source
nodes with `BindingGraph::sources()`. Create a disconnected input with
`graph.pending_source()`, then attach, retarget, or suspend it using
`BindingGraphs::set_source(node, Some(path))` or `set_source(node, None)`.
Retargeting keeps the graph and its computation state intact. Before installation,
use `graph.set_source(...)` directly. The deferred command boundary is unchanged.

The Devices and Wi-Fi views in `crates/empathic/src/core/ui/builder.rs` now use
the checked methods. Dynamic values need explicit shape annotations:

```rust,ignore
// value contains an Id; navigate to that record's Device component.
path!(device_view_entity,
    ReactiveView.value as Id -> Device.wifi_configs)

// value contains the Wi-Fi configuration itself; no entity jump here.
path!(wifi_config_view_entity,
    ReactiveView.value as WifiConfig.password)
```

`-> Component` appends the component name and checks that the preceding typed
value is an Id (or Option<Id>). `as Type` supplies a type for checking subsequent
fields and appends no component name. It is not a runtime cast. Actual dynamic
contents, reflected field registration, Id resolution, and list bounds are still
runtime concerns. Nested Options can be written `Type.settings?.volume`; the
path omits the `?` because the walker already unwraps intermediate Options.
Literal indices (`items[0]`) and tuple fields (`position.0`) are supported.

For computations, `binding_node!` returns a graph node and permits an existing
node, filter, ECS system, or another property path in the chain:

```rust,ignore
let mut graph = BindingGraph::new();
let filtered = binding_node!(graph;
    source(app_entity, AppView.devices)
    => filter::<Id>(|id| *id != Id::nil())
)?;
// Arbitrary multi-input operators stay ordinary Rust calls.
let combined = graph.process((filtered, other_devices), |a: Vec<Id>, b: Vec<Id>| {
    let mut seen = std::collections::HashSet::new();
    Ok(a.into_iter().chain(b).filter(|id| seen.insert(*id)).collect::<Vec<_>>())
})?;
let selected_name = binding_node!(graph;
    node(selected_agent)
    => jump(User.name)
    => map_value::<String, String>(|name| Ok(format!("Hello, {name}")))
)?;
```

`path(Type.field)` projects a returned value; `jump(Component.field)` follows
an Id. `system(name, function)` passes the previous value in `BindingInputs` and
allows read-only Query/Res parameters. It can also start a pipeline with no
explicit graph inputs. `then(function)` is an open extension point receiving
`(&mut BindingGraph, BindingNode)` and returning `BindingResult<BindingNode>`.
This can connect the current result to any multi-input/custom operator. Pipeline
construction is fallible, not transactional: discard the unfinished graph if a
stage fails. Nodes already added are not automatically rolled back.

List rows expose every item directly in `ReactiveView.value`, an owned `Dynamic`
reflected value. Struct field paths remain unchanged. Read scalar items with
`Uuid::from_reflect(view.value.as_ref())`, or bind from
`path!(row, ReactiveView.value)`. This applies to `bind_list`,
`bind_list_from`, and `bind_list_node`. Row callbacks receive the populated
`ReactiveView` for every item, including duplicate values.

`Dynamic` is a thin wrapper around `Box<dyn PartialReflect>` with cloning and
Serde support. It uses the fork's reflected serializers and preserves the
existing struct-value wire format. As with `DynamicStruct`, custom payload and
collection types must be registered on the receiving side (the `Reactive`
derive does this for application types; otherwise use
`enable_global_type_registration!(YourType)`).

Use `bind_node(graph, node, component_path!(Target.field))?` or
`bind_list_node(graph, node, create_row_system)?` to connect a computed graph to a
builder. These consume/install the graph and require `BindingGraphPlugin`.
They use the explicit snapshot timing described below. Both list builders use
the same `ReactiveListView`, callback, and renderer as `bind_list`; this change
does not replace list rendering or change the Wi-Fi Save/BLE actions.

## Usage

Standalone applications add `BindingGraphPlugin` once; `FluxPlugin` includes it
automatically. It initializes `BindingGraphs` and evaluates graphs
in `PostUpdate`, under `BindingGraphSet`. Install a graph with
`graph.install(world, owner)?` or `commands.bind_graph(owner, graph)`.
The owner must exist; despawning it releases its graphs on the next evaluation.
`BindingGraphs::remove_owner(owner)` releases them immediately.

This example combines two device-list paths and binds their union to an existing
`ReactiveListView`. It assumes that the source components and `ReactiveListView`
are registered as `Reactive`, and that the destination view already has its
`create_entity_func` configured. The enclosing function returns `anyhow::Result`.

```rust,ignore
let mut graph = BindingGraph::new();
let local = graph.source(BindingPath::new(
    local_view, "AppView", Some("devices"),
)?)?;
let remote = graph.source(BindingPath::new(
    remote_view, "AppView", Some("devices"),
)?)?;
let devices = graph.process((local, remote), |local: Vec<Id>, remote: Vec<Id>| {
    let mut seen = std::collections::HashSet::new();
    Ok(local.into_iter().chain(remote).filter(|id| seen.insert(*id)).collect::<Vec<_>>())
})?;
graph.bind(devices, BindingPath::new(
    list_entity, "ReactiveListView", Some("value"),
)?)?;
commands.bind_graph(list_entity, graph);
```

The graph writes list values; the existing `process_reactive_lists` renderer
creates the children. That renderer currently rebuilds children when the list
changes. It is not keyed reconciliation: editable rows should retain a stable
record Id and send an explicit edit command, rather than writing back through a
filtered list index. The shared binding writer now truncates trailing list items
when a shorter list snapshot is assigned.

A function can read ordinary ECS components without requiring them to implement
`Reactive`. `Reactive` registration is required only for reflected path access:

```rust,ignore
let visible = graph.process_system(devices,
    |In((ids,)): In<(Vec<Id>,)>,
     devices: Query<(&DBRecord, &Device)>,
     preferences: Res<DevicePreferences>| -> anyhow::Result<Vec<Id>> {
        let ids: Vec<Id> = ids.into_iter().filter(|id| {
            devices.iter().any(|(record, device)| {
                record.id == *id && preferences.matches(device)
            })
        }).collect();
        Ok(ids)
    },
)?;
```

`DevicePreferences` and its predicate above are illustrative application types.
`process` accepts closures and named functions with typed positional arguments;
`process_system` accepts a Bevy system with a typed input tuple and read-only
`Query`/`Res` parameters. Both return `BindingResult<BindingNode>` at construction
and expect callbacks returning `anyhow::Result<T>` where `T: PartialReflect`.
The graph converts arguments through `FromReflect` and boxes the result.

```rust,ignore
let a = graph.process((), || Ok(10_i32))?;
let b = graph.process(a, |a: i32| Ok(a + 1))?;
let c = graph.process((a, b), |a: i32, b: i32| Ok(a + b))?;
let d = graph.process_system((a, b),
    |In((a, b)): In<(i32, i32)>| -> anyhow::Result<i32> { Ok(a * b) })?;
```

Use a single node, `()`, or a tuple of one through sixteen nodes. System inputs
always use tuples (`In<(T,)>` for one node, `In<()>` for zero). Closures use one
argument per node. If any input is unavailable, the callback is skipped and
the output is unavailable; an actual reflected `Option::None` is still a value.
Conversion errors identify the argument index and expected Rust type. Callback
errors become graph diagnostics and prevent the evaluation's sink writes.
System parameter validation still runs on every evaluation, even when an input
is missing. Resource changes and entity removal continue to refresh results.

Set union, intersection, difference, and concatenation are application functions
inside `process`; there are no dedicated graph combinators for them. `map` and
`system` remain available for input counts chosen at runtime, custom handling of
missing values, and untyped reflected data. Computations must not perform I/O
or world writes; system deferred writes are rejected during installation.

For value-only operations, `map_value::<T, U>` converts through `FromReflect`
and reports incompatible inputs. `filter::<T>` preserves list order. `map` and
`system` accept any number of graph inputs and are the common extension points
for joins, reductions, sorting, fallback handling, and application operations.
Use complete query snapshots, not `Changed<T>` query output: the latter is a
change set, not the complete value of a source.

To resume navigation after a function returns a record Id:

```rust,ignore
let agent_name = graph.path(selected_agent_id, "User.name")?;
```

This uses the existing `OptionalParsedPath`, `EntityResolver`, and `PathWalker`.
Path strings are validated at construction. Lookup uses Flux's Id-to-Entity
mapping, never SurrealDB directly. Short component names retain the existing
Flux convention and must be unambiguous. An `Entity` can be a root, but embedded
entity jumps currently follow the existing walker's `Id` semantics.

## Two-way editing

```rust,ignore
graph.two_way(
    BindingPath::new(model, "WifiConfigView", Some("is_editing"))?,
    BindingPath::new(editor, "Toggle", Some("value"))?,
    BindingConflict::Reject,
);

graph.two_way_with(
    BindingPath::new(model, "Settings", Some("count"))?,
    BindingPath::new(editor, "InputField", Some("text"))?,
    BindingConflict::Reject,
    |count: i32| Ok(count.to_string()),
    |text: String| Ok(text.parse::<i32>()?),
);
```

The source wins initial synchronization. Later, either endpoint can change.
When both change to inconsistent values, `Reject` leaves them untouched and
exposes a diagnostic; `SourceWins` and `TargetWins` are explicit alternatives.
Reject persists until values are reconciled or the graph is replaced. A missing
endpoint pauses the link. A changed entity route resets its baseline and starts
source-authoritative synchronization, discarding a stale destination edit.

Conversions must be deterministic and support round trips. For exact mappings,
`backward(forward(source)) == source` and
`forward(backward(target)) == target`; formatting/parsing may deliberately
normalize values. Invalid edits return an error, preserving the model. Endpoint
values require reflected equality; register `#[reflect(PartialEq)]` for opaque
custom values where appropriate. Values such as NaN do not provide useful
reflexive equality and should be normalized or rejected by a converter.

After a successful write, the link stores the actual values on both endpoints.
This suppresses echo updates and accommodates normalization. Two-way links are
planned against the graph's pre-write state and committed after its ordinary
sinks. Derivations see a two-way edit on the next evaluation; there is no
unbounded fixed-point loop.

## Execution, errors, and ownership

- Node handles include graph identity. Foreign handles are rejected. Nodes can
  depend only on earlier nodes, giving deterministic topological evaluation and
  no cycles inside the computation graph. Shared nodes run once per evaluation.
- Each graph computes its values and two-way write directions before any of its
  destinations run. A computation/conversion/conflict error skips its writes.
  Other installed graphs continue. Diagnostics are available through
  `BindingGraphs::errors()` and logged when they change; success clears them.
- Destination writes are sequential, not transactional. A destination error can
  occur after earlier destinations were applied. Reflection can partially apply
  a compound value before returning a type mismatch. Use compatible destination
  types and separate edit commands for transactions involving multiple records.
- Each writable field should have a single owner. Do not simultaneously target
  it with a direct binding, a graph sink, and a two-way link. Direct-only graphs
  settle before computations, and direct consumers of writes are queued again
  within the same update. Computed graphs and two-way links run once, in install
  order; they are not repeatedly executed to seek a fixed point. Connect dependent
  computations within one graph when they need a shared snapshot.
- `None` in `BindingValue` means unresolved/missing. It propagates through the
  supplied path/list/typed operations; a custom `map` can recover it. A missing
  source retains the target. `Some(Box::new(None::<T>))` is a real Option value,
  subject to the existing writer's Option conversion rules. Use an explicit
  empty list to clear a list destination.
- Destination equality is checked before taking a mutable component borrow.
  Equal values do not retrigger list rebuilding. Types without reflected equality
  may be written each update; provide equality for frequently bound values.
- Systems and local state are owned by the graph, not registered as independent
  system entities. Dependency indexes and cached paths are maintained by the runtime.
  Bevy `Local` persists; closures must not hide side effects in it. Graphs cannot
  query their own `BindingGraphs` registry during evaluation because it is scoped
  out of the world while the runtime holds it.

Order PostUpdate consumers after `BindingGraphSet` to observe current deliveries.
`FluxPlugin` schedules its list renderer after that set. Empathic no longer runs
the old scheduler in PreUpdate, Update, and PostUpdate. Update-stage consumers
observe the previous PostUpdate result; applications needing current results
should order their consumer in PostUpdate after the set.
This plugin does not require the database to be connected, but path operators
need the `DBConfig` resource and registered reactive component types. Pure or
ECS-only graphs can run without it. Missing system resources become diagnostics.

## Extension boundaries

This is a runtime Rust graph, not a serialized Blueprint editor. Closures and
Bevy system instances are not persistable graph definitions. A future editor
should use versioned operation names, stable node/port IDs, parameter schemas,
and registered factories that compile into this runtime. Never serialize
`SystemId`, `TypeId`, or process-local `Entity` values as cross-peer identities.

Path reads are incremental: equal paths share cached snapshots. The cache checks
the added/changed ticks of every traversed component and the exact record mappings
it consulted, including unresolved mappings and missing components. Unchanged
paths avoid reflected cloning/walking. A component tick means "possibly changed";
field equality decides whether a source-change delivery is necessary. Route
changes also trigger delivery, even if the new record contains an equal value.
Explicit per-component generations invalidate caches for runtime writes, including
multiple writes within one Bevy tick. Removed owners release their cached paths.
No binding descriptor entities or legacy source/target indexes are allocated.

Direct cascades use a dependency-indexed work queue, with each graph bounded to
`max(64, 1024 / graph_count)` visits per update. Exhausting a graph's budget reports
a diagnostic instead of hanging or starving unrelated graphs. Cycles with types that lack reflected equality
may not be able to establish convergence.

Arbitrary functions and ECS computations still run once per update. Bevy's
component-access declarations describe
permitted access, not the exact set of entities a function actually read. Until
that tracking exists, skipping arbitrary ECS functions because their explicit
inputs compare equal would make results stale.

Async providers should publish loading/ready/error state into ECS resources or
components; graph nodes read those snapshots. Provider requests and database
mutations remain explicit actions in their owning crates. A serialized operation
registry, incremental computation scheduling, keyed list rendering, and multi-record edit
transactions are separate extensions, not guarantees of this implementation.

## Validation of the checked builder migration

The binding tests cover generated strings for nested fields, Option access,
indices, tuple fields, dynamic shapes and Id jumps; entity expressions evaluated
once; pipeline filters, ECS parameters and custom composition; and direct graph
compilation. Builder regression tests run the shared runtime and
list renderer to check unsaved edits, cascading writes, inactive sources, row
callbacks, filtered lists and shrinking lists. Rustdoc checks a valid external
macro invocation and rejects a misspelled field and a non-Id entity jump.

Unified-runtime tests also check idle path-walk counts, long reverse-registered
cascades, once-per-update systems, field-level equality, missing resources and
components, equal-valued record retargeting, graph-native inspection/retargeting, and bounded
cycles without reflected equality.

The focused command is:

```sh
CCACHE_DISABLE=1 cargo empathic test --platform linux --preset server -- --offline -p flux -p flux_derive --lib binding
cargo empathic test --platform linux --preset server -- --offline -p flux --doc ComponentBindingPath
cargo empathic check --platform web --preset client -- --offline -p flux
```

`CCACHE_DISABLE=1` is only needed where the execution sandbox cannot write the
machine's C compiler cache. These tests exercise ECS and renderer behavior; they
are not a browser interaction or hardware provisioning test. The unrelated
network-event deserialization test failure identified during the initial graph
work remains outside this migration.
