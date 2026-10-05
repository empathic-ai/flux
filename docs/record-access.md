# Independent registration and schema contracts

## Stable identities and renames

Source UUIDs are optional. A type may declare `#[schema(database, id = "...")]`;
use its already-published subject UUID when adding the attribute later. Removing
the attribute does not remove the catalog identity. Published aliases retain the
declaration-name-to-storage-key mapping, including for UUID-derived table keys.

For a type without an explicit UUID, use `#[schema(rename_from = "old::Type")]`
with its previous discovery name (the fully qualified Rust name by default, or
the previous `schema(name = "...")` value). Review and finalize the alias-only
draft. The hint can then be removed. Aliases remain reserved and cannot be
reassigned to another subject; renaming never implicitly moves a database table.

Fields accept `#[schema(id = "...")]` with a UUID string. The catalog associates
that UUID with an automatically allocated local integer key; callers never need
to allocate integers. Existing integer-only histories retain their serialization
and fingerprints. Attaching a UUID to an existing field adds a contract revision;
removing its source attribute retains the mapping without another revision.
UUIDs survive field renames and remain reserved after removal. Replacing a
published UUID is rejected with the expected and supplied UUIDs. Without a source
UUID, simultaneous renames need `rename_from` to preserve identity.

These mappings cover managed schema contracts and database table resolution.
They do not make legacy reflection-based replication version-aware. Historical
wire support requires the versioned RPC or managed graph paths and reviewed adapters below.

`add_reactive::<T>()` is local binding only. `add_persistent_record::<T>()`
installs persistence without record request handling or replication.
`add_network_record::<T>()` installs in-memory request handling and replication
without requiring a database connection. `add_record::<T>()` composes both;
repeated registrations do not duplicate these systems. Combined records still
wait for database readiness, while network-only records use session readiness.
Read policies and authenticated-peer checks apply to network-only records too.
Registration never grants access or automatically opts a type into schema
history. Explicit database write APIs are not disabled by these scheduling
choices.

`WireServiceRegistry::expose_versioned::<Request>(contracts)` registers a
separate `.schema` operation alongside an existing legacy operation.
`RemoteServiceClient::call_versioned(request, contracts)` uses the same bounded
transport, cancellation, peer context, and deadlines. `ServiceContracts` holds
independent request, response, and application-error histories. Types must derive
`Schema`, and their current descriptors must match the latest contracts.
Historical payloads are validated by subject, revision, and fingerprint before
upgrading and deserializing into current Rust types. Responses and errors are
downgraded to the caller's requested contracts. Unavailable downgrade paths are
rejected before invoking the handler. Current support is for complete named
JSON objects; patch adaptation is separate and is not automatically applied to
record replication. Unregistered dynamic reflected events remain unversioned.

## Managed event graphs

### Opt-in versioned persistence and projection

`schema::storage::VersionedRecordBackend` defines load, compare-and-swap, and
revision-checked deletion. `SurrealRecordBackend` stores graph payloads and revisions
together in `_flux_versioned_records`, keyed by subject UUID and record UUID.
`Database` exposes these operations as `load_versioned_record`,
`compare_exchange_versioned_record`, `patch_versioned_record`, and
`delete_versioned_record`. These are trusted server APIs, not authorization checks.
Authorize the principal, record, and writable fields before calling them.

Creation uses `expected_revision: None`; updates require the exact positive stored
revision. Writes require the current published graph. Historical patching adapts
both the stored value and patch before atomically comparing and advancing the
revision. Conflicts fail without overwriting the winning writer. Deleted records
retain tombstones and payloads; this is logical deletion, not data erasure.
Recreation requires the tombstone revision and advances it rather than resetting it.

`StoredRecord::replica` constructs a bounded snapshot or delete envelope for an
authorized subscriber. `ReplicaProjection<T>` validates envelopes and deserializes
before updating ECS, replaces complete components, removes the projected component
on deletion, and clears its tracked components when reset to a fresh epoch. It
retains `DBRecord` identities and unrelated components. The transport must verify
the authoritative sender before invoking `apply_authoritative`; the projection is
not a peer-authentication mechanism.

These APIs do not redirect existing per-type tables, `DbQuery`, or reflected
subscriptions. Do not register competing legacy and versioned writers for the same
record. Application adoption requires an explicit migration of existing data and
coordinated routing of reads, writes, subscriptions, and reconnects. The schema
maintenance transaction now also adapts stored historical graphs, checks the
scanned records for concurrent changes, and atomically commits their replacements
with the per-type migrations and ledger. Changed graphs advance record revisions;
tombstones remain deleted and idempotent maintenance does not advance revisions.
Readiness verification rejects stale or invalid stored graphs. This currently
requires at most 4096 records and 16 MiB of combined original/replacement data;
larger stores fail closed pending a bounded batch maintenance implementation.

Nested named structs derive `Schema` independently of `Reactive`. Discovery starts
at explicit database/wire roots and follows nested references, including recursive
ones. Nested descriptors retain catalog identities without acquiring root
capabilities. Standard UUID maps and Bevy platform UUID maps are supported,
including schema-only configurations without the full `bevy` feature.

`Catalog::graph_for::<T>()` verifies reachable Rust descriptors against the
published catalog. `GraphValue::encode_typed` and `decode_typed` validate exact
reachable contract revisions and fingerprints before typed deserialization.
Graph fingerprints include nested revisions even when the parent is unchanged.
Graph decoding is bounded by bytes, reachable types, value depth, and node count.
Patch adaptation does not insert missing-field defaults; collection patches are
whole replacements whose entries must be complete.

`register_managed_event!(EventType, catalog_provider)` explicitly routes a
reflected event through typed graph encoding. The provider returns a published
`Catalog`. The shared network codec rejects legacy Postcard envelopes for these
registered types, unknown subjects, and invalid contract fingerprints. Other
events retain their existing Postcard path. Transports must use
`serialize_network_event` and `deserialize_network_event`, not direct Postcard
calls, to enforce this boundary. Authorization and trusted peer assignment remain
transport/handler responsibilities.

These APIs do not automatically version application record replication.
Database roots containing named references retain append-only `database_graphs`
snapshots. A nested-only edit appends a graph snapshot without incrementing the
parent contract revision. `database::plan_catalog` resolves historical native
migrations against these snapshots, including embedded structs, lists, string
maps, UUID maps, and optional values. Native migrations and their ledger updates
remain one transaction. Unrelated top-level database fields are preserved;
unknown fields inside managed nested objects are rejected.

Existing scalar SQL and ledger definitions remain unchanged, including when a
later revision introduces the first nested field. Native recursive schema graphs
are not supported: depth and generated-query size limits fail closed. Nonempty
object defaults for named references still require a graph-pinned default policy
and are rejected; null optional defaults and empty collection defaults work.
Custom Rust transforms remain wire-only.
Reviewed adapters support versioned integer, enum, and container transforms,
plus registered digest-pinned trusted Rust transforms with required fixtures.
Database lowering supports checked integer conversion with transactional range
validation, explicit scalar enum maps, and recursive option/list/map transforms.
UUID values and UUID-map keys are validated, including equivalent duplicate keys.
Finite `f32`/`f64` fields and floating-point defaults are supported; native checks
reject values outside the declared floating-point range.
Lossy enum maps require maintenance data-loss approval; custom native transforms
and payload-bearing enums remain unsupported.

The backend-neutral `schema::storage::MigrationBackend` interface owns planning,
verification, and application contracts. Implementations must atomically update
records and history and reject unsupported operations. The existing
`schema::database::SurrealMigrationBackend` implements that interface without
changing published SurrealDB ledger definitions. Its SQL plans are backend-specific;
the catalog, identities, and reviewed adapters are not. Existing `schema::database`
functions remain available for compatibility.

## Service registration

Service methods may opt into a shared published contract provider:

```rust,ignore
#[service]
pub trait SessionApi {
    #[wire("empathic.session.info.v1")]
    #[contracts(session_contracts)]
    async fn session_info(request: GetSessionInfo) -> Result<SessionInfo, SessionInfoError>;
}
```

The provider returns `ServiceContracts`. The generated remote client uses
`call_versioned`, and handler registration exposes only the `.schema` operation.
Invalid embedded contracts fail at registration, before serving requests.
Unversioned service declarations retain their existing behavior. Versioned
payload decoding rejects duplicate JSON keys recursively before adaptation.

UUID fields (including Flux `Id`) and unit enums are supported. Standalone enum
contracts require `#[serde(tag = "kind")]`; ordinary string-serialized unit enums
may derive `Schema` as nested scalar values without registering a root. Simple
variant `serde(rename)` is supported and duplicate serialized names are rejected.
Payload-bearing enums, variant identity attributes,
and automatic enum evolution are not supported. The database planner refuses
UUID fields until an explicit native-value adapter is available.

## Replica state machine

`ReplicaEnvelope` represents snapshots, revision-checked patches, and tombstones
separately. `ReplicaEnvelope::from_json` bounds encoded size and rejects duplicate
keys. `ReplicaCache` is scoped to one root graph, a non-nil session epoch, and an
explicit record capacity. Tombstones consume capacity until resynchronization so
delayed snapshots cannot resurrect deleted records. A strictly newer authoritative
snapshot may intentionally recreate a record.

`apply_authoritative` must only be called after transport authentication and record
authorization. It is not a client-write API and does not grant permissions. It
rejects stale revisions, gaps in patch revisions, patches without snapshots,
patches against tombstones, and writer-projection changes. Complete snapshots use
historical complete-value adaptation; patches never insert migration defaults.
Nested object patches merge into existing values; collection fields replace whole
collections. Every resulting record is validated before cache state changes.

Changing the negotiated target graph or reconnecting requires `reset` with a fresh
server-agreed epoch and a fresh snapshot stream. Old-epoch messages are rejected.
The caller must also invalidate its ECS/persistent projections and enforce current
authorization. This library state machine is tested, but is not yet connected to
the application's legacy `AddComponentEvent`/`DbReceiveEvent` handlers, subscriptions,
or database writes. Those paths remain unversioned. No server concurrency or
durable idempotency guarantee is implied by an in-memory replica cache.

`GraphRef::apply_checked_patch` adapts a historical patch against a complete current
record only when the expected record revision matches. It returns a validated
candidate and incremented revision without mutating the original. Authorization,
field ownership, and an atomic compare-and-swap of the record and revision remain
the server handler's responsibility. Never read a revision, compute a candidate,
and then perform an unconditional database replacement.

# Database readiness

Flux owns the application-scoped `DatabaseState`: `Connecting`, `Connected`,
`Preparing`, `Ready`, `Failed`, and `Disconnected`. `Connected` only means the
connection exists. Schedule normal database work on `Ready`.

Register an optional `DatabasePreparation` resource before startup to prepare the
connection. Its async callback receives the existing `Database` handle. Flux
awaits it before declaring readiness; an error transitions to `Failed`. With no
callback, connection success proceeds to `Ready` without preparation. Empathic
registers its server restrictions and migration declarations in its server plugin;
clients do not execute privileged server preparation.

Automatic persistence and replication require both `DBConfig` and `Ready`.
Explicit database calls must also be scheduled appropriately by their caller.
Preparation completion is ignored if the connection has been replaced or the
state has left the connected/preparing phases. This is not an automatic reconnect
implementation. `DbState` remains a compatibility alias for `DatabaseState`;
its `Connected` variant no longer denotes application readiness.

# Record access policies

Unconfigured add_record registrations deny network reads and client writes while
retaining automatic persistence. Use add_record_with_policy to declare a
replication boundary:

    app.add_record_with_policy::<BillingAccount>(RecordPolicy::owner_read_only());
    app.add_record_with_policy::<PrivateCredentials>(RecordPolicy::server_only());

OwnerByRecordId supports per-user tables whose record ID is the authenticated
User ID. It permits reads and server-pushed updates only to peers whose trusted
principal matches that ID. ServerOnly forbids network reads. Both constructors
reject incoming client writes and disable automatic persistence on replicas.
They are intended for services that explicitly persist before publishing an ECS
record change. Public records can retain automatic persistence separately.

AuthenticatedRecordPeers is populated by the server's verified login code. Never
bind a principal from a record, asserted user ID, or unverified network event.
Remove the peer on logout/disconnect and replace its mapping on account changes.
The read check is repeated after asynchronous loading, immediately before sending.
Owner-only change replication also checks the current mapping for each update.
Already delivered data cannot be retracted from a recipient.

Client-side legacy snapshots are accepted only from the authoritative server
peer (Id::nil()). Server-side legacy reception rejects all DbReceiveEvent writes,
including policies with `client_writes` enabled: an unversioned replacement cannot
preserve fields unknown to an older writer. Use authorized typed commands or
revision-checked patches instead. Services must publish through trusted ECS APIs
after persistence. Transport
adapters must assign the sender ID themselves rather than trusting the envelope.

Replication restrictions do not replace database authorization. If clients have
direct database connections, call restrict_record_table::<T> with server credentials
to set table permissions to NONE, and route protected operations through the server.
The helper preserves records, requires schema permissions, and prevents ordinary
anonymous/record-user access. Administrative DB credentials bypass table permissions
and must never be exposed to clients. Broader role/relationship rules and revocable
client caches are future extensions; this API currently provides public,
owner-by-record-ID, and server-only visibility.

## Database migrations

`Migration` declarations are application-owned; `run_migrations_on` executes them
inside Flux. The current runner is for coordinated maintenance: stop old writers
before starting a release that removes fields. It is not a rolling-deployment
protocol and does not prevent an old binary from writing after migration.

The supplied migration list is the complete ordered history. IDs, order, and
generated definitions are immutable once applied. Unknown, reordered, or changed
history fails closed. The ledger stores full definitions rather than hashes.
Changing the SQL generator changes those definitions and must be treated as a
migration compatibility change, not an incidental refactor.

All pending steps and ledger entries execute in one transaction. Field moves
retain record keys and native database values. An existing target must match the
source field exactly; otherwise the transaction fails without deleting the
source. An empty source table is supported, but tables must already exist.
Concurrent runners contend on a shared state record; a losing runner may fail
and must be retried. Do not ignore a migration error and start serving traffic.

The database-backed integration test uses `FLUX_MIGRATION_TEST_URL`, a unique
database under `flux_tests`, and `--test migrations -- --ignored`. Only point it
at a disposable server. It covers conflicts, rollback, native values, repeated
execution, drift, empty tables, and concurrent attempts followed by retry.
Validation used the pinned Rust SDK with the installed 3.1 nightly server, not
an independently built server from the SDK's exact fork revision.

Back up valuable data before maintenance. The runner has no automatic rollback
command, paginated backfill, or live-writer compatibility bridge. Large datasets
need a separately designed resumable backfill rather than one large transaction.

## Automatic persistence and bindings

Automatic persistence and owner replication run after `BindingGraphSet` in
`PostUpdate`, so they observe values produced by that frame's binding evaluation.
Systems that mutate records later in `PostUpdate` need explicit ordering if their
changes must be captured in the same update.

Binding path revisions describe value and route changes. They are not database
commit versions. Persistence retains its own ECS resource for pending snapshots
and acknowledgements; an automatic write completion never reapplies its older
snapshot to the domain component. Ordinary component mutations outside bindings
are still detected.

With the `surrealdb` feature, register a record with custom timing:

```rust,ignore
app.add_record_with_timings::<MyRecord>(
    RecordWriteTiming {
        snapshot_window: std::time::Duration::from_secs(1),
        ..Default::default()
    },
);
```

The helper returns `&mut App` for chaining and preserves existing access policies.
It registers the record itself; do not also call another registration helper for
the same type. `RecordWriteTimings::set` remains available for changing timing
on an already registered record.

The window begins at the first pending change and is not extended by subsequent
changes. Defaults are a zero snapshot window, a five-second retry delay, and
limits of 64 write starts per update and 64 concurrent writes per record type.
An individual record has at most one automatic write in flight. Newer changes
remain pending when an older snapshot succeeds; failures retain the latest
pending value. These are scheduling controls, not persistence deadlines.

This queue is for replaceable snapshots, not append-only events or usage deltas.
It remains memory-only and currently clones changed components each update.
Limits are per type, not database-wide, and dispatch has no explicit fairness
guarantee. Explicit database-first writes do not yet share this queue: do not mix
the two write paths for a record and assume ordering. Deletion, task cancellation,
shutdown flushing, and competing server writers need separate lifecycle handling.
Changing timing does not enable persistence for a policy that disables it.

`add_reactive` registers binding/reflection support without registering record
persistence. It accepts `Component + Reactive + GetTypeRegistration`; local UI
types do not need `Serialize`, `Deserialize`, or the full `FluxRecord` contract.
It does not initialize database configuration or record policies. Deriving
`Reactive` does not opt a type into database migrations or replication.
Full non-persistent replication remains a separate design concern.

The policy tests cover anonymous/other-owner denial, server-only denial, blocked
client writes, account reassignment, and revocation of future recipients. The
server and web builds validate both feature configurations. The broader Flux
editing suite currently has 13 failures in unchanged tests whose setup omits
StatesPlugin; those are separate from these passing policy tests.
