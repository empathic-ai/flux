# Reactive values and collection views

`ReactiveView` is a presentation value slot for **any** reflected value: a
scalar, struct, enum, list, map, or record ID. `ReactiveListView` and
`ReactiveMapView` add a different responsibility: rendering collection entries
as child entities. They are not an exhaustive hierarchy of reflected kinds.

Keep `ReactiveView` for structs too. A separate `ReactiveStructView` is useful
only if it gains specific behavior, such as rendering a generic form by field
name. Splitting the value slot now would require callers to choose different
component paths without providing new behavior.

## Maps

Use `bind_map_from` for a checked path or expression, `try_bind_map_from` to
return construction errors, `bind_map_node` for an explicit graph node, or
`bind_map` for the legacy string-path style. These mirror the list builders.

```rust,ignore
commands.entity(container).builder().bind_map_from(
    path!(source, Settings.labels),
    |In(row): In<Entity>, mut commands: Commands| {
        commands.entity(row).builder().bind_from(
            path!(row, ReactiveView.value),
            component_path!(TextLabel.text),
        );
    },
);
```

Each callback receives a row with both components already populated:

- `ReactiveMapKey.value`: the key, held in `Dynamic`.
- `ReactiveView.value`: the entry value, held in `Dynamic`.

Read them through a query and `FromReflect`, or bind their paths normally.
Keys need the hashing and equality support required by Bevy's `DynamicMap`.
Map iteration order is unspecified. To impose a UI order, compute a sorted
list of entries and use `bind_list_from` instead.

`FluxPlugin` registers the map components and runs `process_reactive_maps`
after `BindingGraphSet`, under the same connected-state condition as lists.
Continue registering `ReactiveView` and source components with `add_reactive`,
as with existing list views. A standalone setup must also register the map
components and schedule the processor itself.

Map snapshots replace the whole `DynamicMap`, so deleted keys and shortened
nested lists do not remain. Unchanged key/value pairs retain their row entities
and descendants, including active editors and focus. Only removed or changed entries are despawned; new or
changed values invoke the renderer again. The container owns all its children.
These rows are presentation snapshots; editing them does not write back to the source. Use an explicit editing adapter for write-back.
Callbacks are skipped for maps with no renderer (for example after deserialization).
Serialization omits callbacks; custom payload types need the same reflected
type registration as `Dynamic`/`ReactiveView`.

## Nested collections

Choose rendering in the row callback. For a map whose values are lists:

```rust,ignore
commands.entity(container).builder().bind_map_from(
    path!(source, Settings.groups),
    |In(row): In<Entity>, mut commands: Commands| {
        commands.entity(row).builder().bind_list_from(
            path!(row, ReactiveView.value),
            render_item,
        );
    },
);
```

This also works for lists of lists or lists of maps: attach the appropriate
builder to the row, binding from `ReactiveView.value`. Attach it to a child
container instead if the row also contains headings or other fixed children.
Nested rendering settles over subsequent schedule updates.

Automatically selecting a component from the reflected kind would still need
layout and callback choices. Keeping that choice explicit supports summaries,
custom editors, and nested collections through one stable row-value path.

The current value/container distinction is sound. A possible future improvement
is updating changed row values in place, with a separate callback contract for
renderers that currently consume snapshots. That is independent of splitting
structs from opaque values or automatically selecting renderers.
