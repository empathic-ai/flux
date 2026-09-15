# Collection and form editing

Implemented in `binding::graph::editing`, re-exported through `flux::prelude::*`.
The existing dataflow and macros are documented in [binding graphs](binding-graphs.md).

## UI policies

| Policy | User editing | Incoming source changes | Commit |
| --- | --- | --- | --- |
| `DisplayOnly` | Session rejects edits; bound text inputs are read-only | Refreshes presentation | None |
| `Live` | Text changes begin an edit automatically | Refreshes between successful edits; preserves unresolved edits | Each valid edit |
| `Manual` | Call `begin()` to start editing | Keeps the visible draft frozen until Save or Cancel | Call `save()` |

`begin()` freezes the baseline immediately, even before the first keystroke.
`EditSession<T>::value()` is the visible value/draft; `baseline()` is its original
value and `latest()` is the separately observed source. An active editor never
refreshes its draft from the source. `cancel()` explicitly reloads the latest
source, or removes the editor if its source item no longer exists.

A successful Save updates the local ECS model. It does not imply database
persistence, queued transport delivery, or device acknowledgement. The row receives
an `EditCommitted` observer trigger after its session and source have been updated. Empathic owns any subsequent persistence or provisioning action.

`FluxPlugin` installs `EditBindingPlugin`. Standalone users add that plugin and
supply the usual `DBConfig` resource and reactive component registrations. No
live database connection is needed for local editing. The plugin runs in `Update`
under `EditBindingSet`; browser input collection is ordered before it.

## Collection identity

Your data can stay in standard Rust collections. Editor state lives on row
entities rather than inside serialized records.

| Adapter | Source type | Identity |
| --- | --- | --- |
| `IndexedVec::<T>::default()` | `Vec<T>` | Original index plus collection snapshot guard |
| `KeyedVec::new(key_fn)` | `Vec<T>` | Explicit unique key |
| `MapEntries::<K, V>::default()` | `HashMap<K, V>` | Original map key |
| `EditValue::<T>::default()` | A scalar or struct `T` | The checked source path |

For keyed collections, refreshing or reordering retains matching row entities and
other active drafts. Maps retain existing display order and append new keys;
`HashMap` itself has no order. Duplicate vector keys produce an error rather than
selecting one occurrence. Changing a keyed vector item's key is rejected.

An index identifies a slot, not a durable item. Indexed editors reject Save if the
collection differs from their captured snapshot, including changes to other items.
They keep the draft so the user can review it or Cancel. This supports existing
`Vec<Struct>` models without silently editing a different item after reordering.
Snapshot comparison cannot detect removal/reinsertion or reorder-and-restore
histories with identical final values. Applications requiring occurrence identity
across those histories need durable keys or mutation tracking in a custom adapter.

A `Vec<Id>` can contain the same ID more than once. `KeyedVec::new(|id: &Id| *id)`
is suitable only if each ID occurs once; otherwise use positional addressing or
supply occurrence keys. Editing a record behind an ID normally targets that
record's checked path, rather than replacing the ID in the collection.

## UUID-keyed Wi-Fi example

Once `Device.wifi_configs` is `HashMap<uuid::Uuid, WifiConfig>`, the list call is:

```rust,ignore
builder.bind_editable_list_from(
    path!(device_view_entity, ReactiveView.value as Id -> Device.wifi_configs),
    MapEntries::<uuid::Uuid, WifiConfig>::default(),
    EditPolicy::Manual,
    |In(row), mut commands: Commands| {
        commands.entity(row).builder().with_children(|parent| {
            parent.child().input_field("Password".into(), InputType::Password)
                .bind_edit_input(
                    row,
                    |config: &WifiConfig| config.password.clone().unwrap_or_default(),
                    |config, text| config.password = Some(text),
                );
        });
    },
);
```

The renderer runs once per row identity. It receives `EditSession<WifiConfig>` and
`EditStatus`; struct items also get a `ReactiveView` presentation snapshot for
existing label/path bindings. Map keys are retained internally; no key field is
required in `WifiConfig`. The application supplies UUIDs when creating entries.

An Edit action calls `editors.get_mut(row)?.begin()?`. Save calls `save()?` and
Cancel calls `cancel()`. These request processing by the next editor update;
requesting Save is not itself confirmation of success. Show the edit panel using
`EditStatus.active`, the non-editing view using `EditStatus.viewing`, and errors
using `EditStatus.message`. `EditStatus.commits` counts successful local commits.

The current Empathic model remains `Vec<WifiConfig>` pending the user's separate
UUID-map migration. Its Devices UI uses `IndexedVec<WifiConfig>` now; switch that
one adapter to `MapEntries<uuid::Uuid, WifiConfig>` with the model change. The row
editor and password binding remain the same. Server seed values and the integration
test's fixtures will also need to adopt the new collection type.

## Fields, validation, and display-only widgets

Use `bind_edit_from::<T, _, _>(source, policy, renderer)` for a single value or form.
For a plain read-only text field, `input_field(...).bind_read_only_input_from(source)`
is a one-way binding with `InputField.read_only = true`, without an edit session.

`bind_edit_input` connects typed getter/setter closures to an existing session.
Manual fields remain read-only until the session begins. Live fields begin
editing automatically on input and commit each valid edit.

Use `bind_edit_input_try` for fallible conversion, for example:

```rust,ignore
input.bind_edit_input_try(
    row,
    |value: &i32| value.to_string(),
    |value, text| {
        *value = text.parse().map_err(|_| EditError::Invalid)?;
        Ok(())
    },
);
```

Invalid text stays in the input, blocks Save for that session, and exposes
`EditInputStatus.message` on the input entity. Source refreshes cannot erase it.
Cancel discards raw input and reloads the source. Conversion runs on a candidate
clone, so a failing setter cannot partially modify the typed draft. Converters
may normalize valid text; diagnostics never include the input contents.

`bind_validated_editable_list_from` accepts a pure item validator before the
renderer argument. Validation runs before any write. Rejection preserves the
active draft. Lower-level installation is available through `install_edit_binding`,
`install_edit_input`, and `install_edit_input_try`.

The web renderer enforces `InputField.read_only` in both the HTML input and its
input-event handling, and no longer logs entered text (which may be a password).
Programmatic component mutation remains possible; this is a UI contract, not
Rust immutability. Custom widgets/platforms must honor the same policy.

Existing `bind_from`, `bind_component_property`, and `bind_list` retain their
compatibility behavior. They are low-level data propagation, not edit sessions
or read-only UI enforcement. Do not aim one of those bindings or a graph sink at
an input already owned by an edit adapter.

## Filters, sorting, and extension points

Adapters compose without losing source keys:

```rust,ignore
let adapter = KeyedVec::new(|item: &Item| item.id)
    .filter(|item| item.visible)
    .sort_by(|a, b| a.id.cmp(&b.id));
```

Filtering and sorting affect presentation only. Active rows hidden by a filter
remain available to finish editing; Save still resolves their original item.
Use deterministic comparison functions when specifying display order.

Implement `EditCollection` to add collection types or projections. `entries`
returns visible key/value pairs, `get` resolves original items including hidden
ones, and `replace` updates only the addressed item in the current collection.
An adapter must preserve unrelated entries and validate key semantics. It operates
on typed candidates and must have no external side effects.

Arbitrary graph results, unions, and aggregations remain read-only unless an
explicit adapter defines their write semantics. Value equality alone cannot tell
which input of a union an edit should change. Collection membership operations
remain explicit application commands, separate from editing an item's fields.

## Conflicts and limits

- Save compares the current source item with the baseline. A concurrent different
  value causes an error. If the source already equals the draft, a keyed/value
  commit is an idempotent success. There is no automatic field merge.
- An ID route is checked against its captured entity chain. Navigation/retargeting
  cannot redirect an active Save to a different record. Missing items retain active
  drafts; Cancel removes an orphan row. Despawning the owner releases its editors
  and drafts; drafts are not persisted across navigation or application restarts.
- Source reads poll each update. Collection snapshots are shared among rows, but
  polling still clones reflected data; this is not incremental dependency tracking.
- A commit is one local collection write, not a transaction across records or
  multiple editors. Driver/row processing uses entity order for reproducibility.
  Custom reflected setters must honor their types; the underlying reflection writer
  does not supply a general transaction/rollback facility.
- Item/collection equality must be meaningful. Normalize non-reflexive values such
  as NaN before using them in optimistic conflict checks.
- The provisioning `DeviceConfig.wifi_configs` payload is separately keyed by
  SSID. UUID editor keys do not resolve that protocol's inability to represent
  multiple credential entries for the same SSID. The Wi-Fi Save action updates
  the Device ECS record; it does not silently re-enable the old commented network
  code or claim the device applied the credentials.

## Validation

Flux tests cover UUID maps, stable rows, keyed reorder, indexed conflicts,
source deletion/retargeting, duplicate keys, untouched active drafts, validation,
manual and live inputs, read-only sessions, invalid raw text, filtering/sorting,
commit observers, and owner cleanup. Empathic's integration test uses the actual
Device path with duplicate SSIDs and verifies local Save plus another retained
draft. Web compilation checks the browser input changes; physical device delivery
is a separate concern.

Architectural references: [Qt's submission policies](https://doc.qt.io/qt-6/qdatawidgetmapper.html)
and [React's list identity guidance](https://react.dev/learn/rendering-lists).
