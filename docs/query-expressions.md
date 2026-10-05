# Query expressions

Flux query macros build typed query expressions. A query expression describes a
record query, not an execution side effect. The same portable expression can be
interpreted by the local Bevy ECS or by SurrealDB.

The examples below use fictional user-defined types:

```rust,ignore
struct UserRecord {
    code: String,
    key: String,
}

struct Active;
struct Archived;
```

In a real application, these types implement the required Flux traits and
`Active`/`Archived` are Bevy components. They are shown here only to explain
the query API without depending on an application-specific record type.

## Portable syntax

The portable form currently supports a record type, equality and ordering
comparisons joined with `AND`, and a literal `LIMIT`:

```rust,ignore
let expression = query_one!(UserRecord WHERE code = user_code LIMIT 1);
```

The record type supplies the database table name through Flux's existing
`FluxRecord::short_type_path()` convention. The right-hand expression is kept
as a bound value for SurrealDB and as a typed value for the ECS predicate. It
is not interpolated into a query string.

Portable binding expressions are evaluated once and serialized into Flux-owned
JSON values, so they must implement `Serialize`. The original typed value is
used by the ECS predicate. The record field comparison is checked by Rust when
the macro expansion is compiled. Serialization failures are reported at database
execution and do not invoke the handler. Use `bevy_query!` for values without a
portable serialization. Native database-only values belong in raw queries.

## ECS execution

Use `Commands::query` or `Commands::query_one` for local records already
loaded into the Bevy world:

```rust,ignore
fn find_user(mut commands: Commands) {
    let user_code = "USR-123".to_string();
    commands.query_one(
        query_one!(UserRecord WHERE code = user_code LIMIT 1),
        move |In(user): In<Option<(Id, UserRecord)>>, time: Res<Time>| {
            if let Some((user_id, user)) = user {
                info!("Found user {user_id} ({}) after {} seconds", user.code, time.elapsed_secs());
            }
        },
    );
}
```

`query!` invokes its handler with `In<Vec<(Id, T)>>`; `query_one!` invokes its
handler with `In<Option<(Id, T)>>`. The handler is a normal Bevy system
function, so additional `Res`, `ResMut`, `Query`, `EventWriter`, and other
system parameters can follow the input value.

The ECS executor snapshots matching record components before invoking the
handler. `query_one` passes `None` when no record matches or when more than one
record matches. A warning is logged for the multiple-match case.

## SurrealDB execution

Use `Commands::db_query` or `Commands::db_query_one` with the same expression:

```rust,ignore
fn find_user_in_database(mut commands: Commands) {
    let user_code = "USR-123".to_string();
    commands.db_query_one(
        query_one!(UserRecord WHERE code = user_code LIMIT 1),
        move |In(user): In<Option<(Id, UserRecord)>>, time: Res<Time>| {
            if let Some((user_id, user)) = user {
                info!("Found user {user_id} ({}) at {} seconds", user.code, time.elapsed_secs());
            }
        },
    );
}
```

Database execution uses the generated table name and bound variables, then
runs the handler back on the Bevy world. The handler receives the same plain
value shape as ECS execution. Database failures are logged by Flux; the handler
is not invoked on failure.

## Bevy-specific predicates

Use `bevy_query!` or `bevy_query_one!` when the predicate is ECS-only or more
expressive than the portable recipe language:

```rust,ignore
commands.query(
    bevy_query!(UserRecord, |record: &UserRecord| {
        record.code.starts_with("USR-") && record.key.len() > 8
    }),
    move |In(users): In<Vec<(Id, UserRecord)>>,
          time: Res<Time>| {
        let _elapsed = time.elapsed_secs();
        let _users = users;
    },
);
```

This escape hatch is ECS-only and is not converted into SurrealQL. It is useful
for predicates involving Rust methods, local state, or values that have no
database representation.

Structural Bevy filters can be supplied before the predicate. They are applied
by Bevy before the Rust predicate runs:

```rust,ignore
commands.query(
    bevy_query!(
        UserRecord,
        (With<Active>, Without<Archived>, Changed<UserRecord>),
        |record: &UserRecord| record.code.starts_with("USR-")
    ),
    move |In(users): In<Vec<(Id, UserRecord)>>| {
        let _users = users;
    },
);
```

`With<T>`, `Without<T>`, `Changed<T>`, and tuples of Bevy query filters are
supported through the ECS query itself. The filter form is ECS-only and is not
included in the generated SurrealQL.

The portable `query!(UserRecord WHERE ...)` form is the one that has a `WHERE`
clause. `bevy_query!` deliberately uses a Rust predicate instead, because its
purpose is to expose ECS-specific behavior that cannot be represented by the
shared database syntax.

For SurrealDB features that are not part of the portable expression language, use
the raw escape hatch:

```rust,ignore
commands.db_query_raw(
    "SELECT * FROM user_record WHERE code = $code ORDER BY created_at DESC LIMIT 1",
    flux::surrealdb_client::types::vars! { code: user_code },
    move |In(users): In<Vec<(Id, UserRecord)>>| {
        // Handle the users.
    },
);
```

The raw methods are named `db_query_raw` and `db_query_one_raw` to keep the
portable `query` and `query_one` names reserved for ECS execution.

## Backend boundary

The shared expression deliberately covers only semantics that can be represented
by both backends. ECS-specific filters and predicates stay in `bevy_query!` so
the API does not imply that SurrealDB has the same execution model.

`QueryPlan` retains field comparisons, indexed parameter names, and limits;
`QueryBindings` holds portable serialized values. Macros do not emit SQL or refer
to the SurrealDB SDK. Repeated comparisons on one field receive separate bindings.
The SurrealDB adapter validates identifiers and lowers the plan only when the
database executor is selected. Table resolution uses the published schema catalog
when a record has a managed identity.

This is an extensible query representation, not a claim that runtime backend
selection is complete. `DBConfig`, raw execution, and persistence still use the
SurrealDB implementation. Another backend must implement execution and agree on
value semantics, including absent versus null, UUID encoding, integer ranges, and
cardinality. The existing raw methods deliberately remain SurrealDB-specific.
