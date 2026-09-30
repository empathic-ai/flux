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

Client-side protected updates are accepted only from the authoritative server
peer (Id::nil()). Server-side protected updates reject all DbReceiveEvent writes;
services must publish through trusted ECS APIs after persistence. Transport
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
persistence. Full non-persistent replication remains a separate design concern.

The policy tests cover anonymous/other-owner denial, server-only denial, blocked
client writes, account reassignment, and revocation of future recipients. The
server and web builds validate both feature configurations. The broader Flux
editing suite currently has 13 failures in unchanged tests whose setup omits
StatesPlugin; those are separate from these passing policy tests.
