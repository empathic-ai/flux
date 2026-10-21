use crate::prelude::*;
use anyhow::{Error, anyhow};
use bevy::prelude::*;
use bevy_async_ecs::*;
use bevy_wasm_tasks::*;
use common::prelude::*;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use surrealdb::{Surreal, engine::any::Any, opt::auth::Root, types::SurrealValue};

pub fn compile_surreal_query(plan: &QueryPlan, table: &str) -> anyhow::Result<String> {
    fn identifier(value: &str) -> anyhow::Result<&str> {
        anyhow::ensure!(!value.is_empty() && value.chars().all(|character| character.is_alphanumeric() || character == '_'), "Invalid query identifier: {value}");
        Ok(value)
    }
    let mut statement = format!("SELECT * FROM `{}`", identifier(table)?);
    for (index, condition) in plan.conditions.iter().enumerate() {
        statement.push_str(if index == 0 { " WHERE " } else { " AND " });
        let operator = match condition.comparison {
            QueryComparison::Eq => "=", QueryComparison::Ne => "!=",
            QueryComparison::Lt => "<", QueryComparison::Le => "<=",
            QueryComparison::Gt => ">", QueryComparison::Ge => ">=",
        };
        statement.push_str(&format!("`{}` {operator} ${}", identifier(&condition.field)?, identifier(&condition.parameter)?));
    }
    if let Some(limit) = plan.limit { statement.push_str(&format!(" LIMIT {limit}")); }
    Ok(statement)
}

pub(crate) fn lower_surreal_query<T: FluxRecord, V: QueryParameters>(
    plan: Option<QueryPlan>, variables: V,
) -> anyhow::Result<(String, surrealdb::types::SerdeWrapper<std::collections::BTreeMap<String, serde_json::Value>>)> {
    let plan = plan.ok_or_else(|| anyhow!("ECS-only query cannot execute against a database"))?;
    let table = crate::schema::database::record_table::<T>(T::short_type_path())?;
    Ok((compile_surreal_query(&plan, &table)?, surrealdb::types::SerdeWrapper(variables.into_parameters()?)))
}

#[derive(Clone)]
pub struct Database {
    connection: Arc<Surreal<Any>>,
}

#[derive(Default)]
pub struct RecordCreateBatch {
    records: Vec<(String, String, surrealdb::types::Value)>,
}

impl RecordCreateBatch {
    pub fn push<T: FluxRecord>(&mut self, id: Id, record: T) -> anyhow::Result<()> {
        anyhow::ensure!(id != Id::nil(), "Missing record identity");
        anyhow::ensure!(self.records.len() < 64, "Record creation batch exceeds limit");
        self.records.push((
            crate::schema::database::record_table::<T>(T::short_type_path())?,
            id.to_pretty_string(),
            surrealdb::types::SerdeWrapper(record).into_value(),
        ));
        Ok(())
    }
}

#[derive(Resource, Clone)]
pub struct DatabasePreparation(
    pub fn(Database) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>>,
);

pub fn prepare_database(
    mut commands: Commands,
    config: Res<DBConfig>,
    preparation: Option<Res<DatabasePreparation>>,
    mut next: ResMut<NextState<DatabaseState>>,
) {
    let Some(preparation) = preparation else {
        next.set(DatabaseState::Ready);
        return;
    };
    let prepare = preparation.0;
    let database = config.database();
    next.set(DatabaseState::Preparing);
    commands.run(async move |world| {
        let state = match prepare(database.clone()).await {
            Ok(()) => DatabaseState::Ready,
            Err(error) => {
                tracing::error!(%error, "Database preparation failed");
                DatabaseState::Failed
            }
        };
        world.apply(move |world: &mut World| {
            if world.get_resource::<DBConfig>().is_some_and(|config| Arc::ptr_eq(&config.db, &database.connection))
                && matches!(world.resource::<State<DatabaseState>>().get(), DatabaseState::Connected | DatabaseState::Preparing)
            {
                world.resource_mut::<NextState<DatabaseState>>().set(state);
            }
        }).await;
    });
}

impl DBConfig {
    pub fn database(&self) -> Database {
        Database { connection: self.db.clone() }
    }
}

impl Database {
    /// Consume one shared fixed-window allowance using database time and an atomic update.
    pub async fn consume_rate_limit(&self, bucket: &str, limit: u32, window_seconds: u32) -> anyhow::Result<bool> {
        use sha2::{Digest, Sha256};
        anyhow::ensure!(!bucket.is_empty() && bucket.len() <= 256, "Invalid rate-limit bucket");
        anyhow::ensure!(limit > 0 && (1..=86_400).contains(&window_seconds), "Invalid rate-limit policy");
        let key = format!("{:x}", Sha256::digest(bucket.as_bytes()));
        let mut response = self.connection.query(
            "DEFINE TABLE IF NOT EXISTS _flux_rate_limits SCHEMALESS PERMISSIONS NONE;
             ALTER TABLE _flux_rate_limits PERMISSIONS NONE;
             RETURN {
                 LET $now = time::unix(time::now());
                 LET $expired = SELECT VALUE id FROM _flux_rate_limits WHERE expires_at <= $now LIMIT 64;
                 FOR $record IN $expired { DELETE $record WHERE expires_at <= $now; };
                 LET $used = UPSERT type::record('_flux_rate_limits', $key)
                     SET count = IF expires_at = NONE OR expires_at <= $now { 1 } ELSE { count + 1 },
                         expires_at = IF expires_at = NONE OR expires_at <= $now { $now + $window } ELSE { expires_at }
                     WHERE count = NONE OR expires_at <= $now OR count < $limit RETURN AFTER;
                 RETURN array::len($used) = 1;
             };"
        ).bind(("key", key)).bind(("limit", i64::from(limit))).bind(("window", i64::from(window_seconds)))
            .await?.check()?;
        let allowed: Option<bool> = response.take(2)?;
        Ok(allowed.unwrap_or(false))
    }

    /// Remove a bounded batch of expired ephemeral records, never other subjects.
    pub async fn purge_expired_versioned_records(
        &self, catalog: &crate::schema::Catalog, subject: &str, expiry_field: &str, now: u64, limit: u32,
    ) -> anyhow::Result<()> {
        anyhow::ensure!((1..=256).contains(&limit), "Invalid expiry batch size");
        anyhow::ensure!(now <= i64::MAX as u64, "Invalid expiry timestamp");
        let root = catalog.roots.values().find(|root| root.history.contracts[0].subject == subject)
            .ok_or_else(|| anyhow::anyhow!("Unknown expiry subject"))?;
        anyhow::ensure!(root.database, "Expiry requires a database root");
        anyhow::ensure!(root.history.contracts.last().unwrap().fields.values().any(|field|
            field.name == expiry_field && field.ty == crate::schema::ValueType::U64
        ), "Expiry requires an unsigned timestamp field");
        self.connection.query(
            "DEFINE TABLE IF NOT EXISTS _flux_versioned_records SCHEMALESS PERMISSIONS NONE;
             LET $expired = SELECT VALUE id FROM _flux_versioned_records
                 WHERE payload.graph.root = $subject
                     AND payload.value[$field] != NONE AND payload.value[$field] != NULL
                     AND payload.value[$field] <= $now LIMIT $limit;
             FOR $record IN $expired {
                 DELETE $record WHERE payload.graph.root = $subject
                     AND payload.value[$field] != NONE AND payload.value[$field] != NULL
                     AND payload.value[$field] <= $now;
             };"
        ).bind(("subject", subject.to_owned())).bind(("field", expiry_field.to_owned()))
            .bind(("now", now as i64)).bind(("limit", limit as i64)).await?.check()?;
        Ok(())
    }

    #[cfg(all(feature = "database_backup", not(target_arch = "wasm32")))]
    pub async fn export_backup(&self, destination: &std::path::Path) -> anyhow::Result<u64> {
        use futures::StreamExt;
        use std::io::Write;
        struct PartialExport(std::path::PathBuf);
        impl Drop for PartialExport {
            fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
        }
        anyhow::ensure!(!destination.try_exists()?, "Backup destination already exists");
        let parent = destination.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
        let temporary = parent.join(format!(".flux-backup-{}.partial", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let partial = PartialExport(temporary);
        let mut stream = self.connection.export(()).await?;
        let mut bytes = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            bytes += chunk.len() as u64;
        }
        file.sync_all()?;
        anyhow::ensure!(bytes > 0, "Database export was empty; do not use it as a backup");
        std::fs::hard_link(&partial.0, destination)?;
        drop(file);
        drop(partial);
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(bytes)
    }

    pub async fn load_versioned_record(&self, subject: &str, record_id: uuid::Uuid) -> anyhow::Result<Option<crate::schema::storage::StoredRecord>> {
        use crate::schema::storage::VersionedRecordBackend;
        crate::schema::database::SurrealRecordBackend(&self.connection).load(subject, record_id).await
    }

    pub async fn compare_exchange_versioned_record(
        &self, catalog: &crate::schema::Catalog, record_id: uuid::Uuid, expected_revision: Option<u64>, payload: crate::schema::GraphValue,
    ) -> anyhow::Result<crate::schema::storage::StoredRecord> {
        use crate::schema::storage::VersionedRecordBackend;
        crate::schema::database::SurrealRecordBackend(&self.connection).compare_exchange(catalog, record_id, expected_revision, payload).await
    }

    pub async fn patch_versioned_record(
        &self, catalog: &crate::schema::Catalog, target: &crate::schema::GraphRef,
        record_id: uuid::Uuid, expected_revision: u64, patch: crate::schema::GraphValue,
    ) -> anyhow::Result<crate::schema::storage::StoredRecord> {
        crate::schema::storage::patch_record(&crate::schema::database::SurrealRecordBackend(&self.connection), catalog, target, record_id, expected_revision, patch).await
    }

    pub async fn delete_versioned_record(&self, subject: &str, record_id: uuid::Uuid, expected_revision: u64) -> anyhow::Result<crate::schema::storage::StoredRecord> {
        use crate::schema::storage::VersionedRecordBackend;
        crate::schema::database::SurrealRecordBackend(&self.connection).delete(subject, record_id, expected_revision).await
    }

    pub async fn create_record_pair<First: FluxRecord, Second: FluxRecord>(
        &self, first_id: Id, first: First, second_id: Id, second: Second,
    ) -> anyhow::Result<()> {
        use surrealdb::types::SerdeWrapper;
        let mut response = self.connection.query(
            "BEGIN TRANSACTION;
             CREATE type::record($first_table, $first_id) CONTENT $first;
             CREATE type::record($second_table, $second_id) CONTENT $second;
             COMMIT TRANSACTION;"
        )
            .bind(("first_table", crate::schema::database::record_table::<First>(First::short_type_path())?))
            .bind(("first_id", first_id.to_pretty_string()))
            .bind(("first", SerdeWrapper(first)))
            .bind(("second_table", crate::schema::database::record_table::<Second>(Second::short_type_path())?))
            .bind(("second_id", second_id.to_pretty_string()))
            .bind(("second", SerdeWrapper(second)))
            .await?;
        let mut errors: Vec<_> = response.take_errors().into_iter().collect();
        errors.sort_by_key(|(index, _)| *index);
        anyhow::ensure!(errors.is_empty(), "Record transaction failed: {}", errors.into_iter()
            .map(|(index, error)| format!("statement {index}: {error}"))
            .collect::<Vec<_>>().join("; "));
        Ok(())
    }

    pub async fn create_record_batch(&self, batch: RecordCreateBatch) -> anyhow::Result<()> {
        anyhow::ensure!(!batch.records.is_empty(), "Empty record creation batch");
        let mut query = self.connection.query("BEGIN TRANSACTION;");
        for (index, (table, key, content)) in batch.records.into_iter().enumerate() {
            query = query.query(format!("CREATE type::record($table_{index}, $key_{index}) CONTENT $content_{index};"))
                .bind((format!("table_{index}"), table))
                .bind((format!("key_{index}"), key))
                .bind((format!("content_{index}"), content));
        }
        query.query("COMMIT TRANSACTION;").await?.check()?;
        Ok(())
    }

    pub fn from_connection(connection: Arc<Surreal<Any>>) -> Self {
        Self { connection }
    }

    pub async fn upsert_record<T: FluxRecord>(&self, id: Id, record: T) -> anyhow::Result<()> {
        upsert_record(&self.connection, id, record).await
    }

    pub async fn create_record<T: FluxRecord>(&self, id: Id, record: T) -> anyhow::Result<Option<T>> {
        use surrealdb::types::SerdeWrapper;
        let created: Option<SerdeWrapper<T>> = self.connection
            .create((crate::schema::database::record_table::<T>(T::short_type_path())?, id.to_pretty_string()))
            .content(SerdeWrapper(record)).await?;
        Ok(created.map(|record| record.0))
    }

    pub async fn restrict_record_table<T: FluxRecord>(&self) -> anyhow::Result<()> {
        restrict_record_table::<T>(&self.connection).await
    }

    pub async fn run_migrations(&self, migrations: &[Migration]) -> anyhow::Result<()> {
        run_migrations_on(&self.connection, migrations).await
    }

    pub async fn verify_schema(
        &self, catalog: &crate::schema::Catalog, legacy: &[crate::schema::database::LegacyEntry],
    ) -> anyhow::Result<()> {
        use crate::schema::storage::MigrationBackend;
        crate::schema::database::SurrealMigrationBackend(&self.connection).verify(catalog, Some(legacy)).await
    }

    pub async fn get_record<T: FluxRecord>(&self, id: Id) -> anyhow::Result<Option<T>> {
        get_record::<T>(&self.connection, id).await
    }

    pub async fn get_records<T: FluxRecord>(&self) -> anyhow::Result<Vec<(Id, T)>> {
        get_records::<T>(self.connection.clone()).await
    }
}

#[derive(Debug, SurrealValue, Serialize, Deserialize)]
pub struct Record {
    #[allow(dead_code)]
    id: surrealdb::types::record_id::RecordId,
}

pub fn start(
    runner: Res<AsyncRunner>,
    tasks: Tasks,
    mut state: ResMut<NextState<DatabaseState>>,
) -> Result {
    state.set(DatabaseState::Connecting);
    //info!("Starting server...");

    #[cfg(all(feature = "server", feature = "production"))]
    {
        info!("Starting database...");

        Command::new("rm")
            .args(["mount/efs/database/LOCK"])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        Command::new("surreal")
            .args([
                "start",
                "file://mount/efs/database",
                "--log",
                "error",
                "--no-banner",
                "--user",
                "root",
                "--pass",
                "root",
                "--bind",
                "0.0.0.0:7777",
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        info!("Started database.");

        tokio::time::sleep(Duration::from_secs(15)).await;
    }

    //runtime.(|_ctx| async move {
    //    println!("This task is running on a background thread");
    //});

    //let rt: tokio::runtime::Runtime = tokio::runtime::Runtime::new()?;

    let async_world = runner.get_async_world();

    tasks.spawn_auto(async move |x| {
        let db = match get_database().await {
            Ok(database) => database,
            Err(error) => {
                tracing::error!(%error, "Database connection failed");
                async_world.register_system(|mut state: ResMut<NextState<DatabaseState>>| {
                    state.set(DatabaseState::Failed);
                }).await.run().await;
                return;
            }
        };

        async_world
            .register_system(
                move |mut commands: Commands, mut state: ResMut<NextState<DatabaseState>>| {
                    commands.insert_resource(DBConfig {
                        db: Arc::new(db.clone()),
                        id_mappings: Default::default(),
                        entity_mappings: Default::default(),
                    });
                    state.set(DatabaseState::Connected);
                    //info!("Set state to connected!");
                },
            )
            .await
            .run()
            .await;
    });

    //bevy::tasks::block_on(fut);

    //AsyncComputeTaskPool::get().spawn_local(fut).detach();
    //common::utils::spawn(async move {
    //*done_clone.lock().unwrap() = true;
    //Ok(())
    //});

    Ok(())
}

static CLIENT_DATABASE_ADDRESS: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn configure_local_database(path: &std::path::Path) -> anyhow::Result<()> {
    anyhow::ensure!(path.is_absolute(), "Local database path must be absolute");
    let path = path.to_str().ok_or_else(|| anyhow!("Invalid local database path"))?;
    let address = format!("surrealkv://{path}");
    if let Some(current) = CLIENT_DATABASE_ADDRESS.get() {
        anyhow::ensure!(current == &address, "Local database is already configured");
        return Ok(());
    }
    CLIENT_DATABASE_ADDRESS.set(address).map_err(|_| anyhow!("Local database configuration raced"))
}

/// Connect to the configured SurrealDB
pub async fn get_database() -> anyhow::Result<Surreal<Any>> {
    #[cfg(all(feature = "client", not(feature = "server")))]
    {
        static CLIENT_DATABASE: std::sync::OnceLock<futures::lock::Mutex<Option<Surreal<Any>>>> = std::sync::OnceLock::new();
        let mut connection = CLIENT_DATABASE.get_or_init(|| futures::lock::Mutex::new(None)).lock().await;
        if let Some(database) = connection.as_ref() { return Ok(database.clone()); }
        let database = connect_database(get_database_address()?).await?;
        *connection = Some(database.clone());
        return Ok(database);
    }
    #[cfg(not(all(feature = "client", not(feature = "server"))))]
    connect_database(get_database_address()?).await
}

#[cfg(feature = "database_backup")]
pub async fn get_backup_database() -> anyhow::Result<Surreal<Any>> {
    let address = get_database_address()?;
    let address = if let Some(rest) = address.strip_prefix("ws://") {
        format!("http://{rest}")
    } else if let Some(rest) = address.strip_prefix("wss://") {
        format!("https://{rest}")
    } else {
        address
    };
    connect_database(address).await
}

async fn connect_database(database_address: String) -> anyhow::Result<Surreal<Any>> {

    #[cfg(feature = "server")]
    let namespace =
        std::env::var("SURREAL_NAMESPACE").map_err(|_| anyhow!("Set SURREAL_NAMESPACE"))?;
    #[cfg(not(feature = "server"))]
    let namespace = "test".to_string();
    #[cfg(feature = "server")]
    let database =
        std::env::var("SURREAL_DATABASE").map_err(|_| anyhow!("Set SURREAL_DATABASE"))?;
    #[cfg(not(feature = "server"))]
    let database = "test".to_string();
    #[cfg(feature = "server")]
    let token = std::env::var("SURREAL_TOKEN").ok();
    #[cfg(feature = "server")]
    let username = std::env::var("SURREAL_USER").ok();
    #[cfg(feature = "server")]
    let password = std::env::var("SURREAL_PASS").ok();

    let db: Surreal<Any> = Surreal::init();

    info!("Connecting to database.");
    if let Ok(_) = db.connect(database_address).await {
        // Signin as a namespace, database, or root user
        #[cfg(feature = "server")]
        if let Some(token) = token {
            db.authenticate(token).await.unwrap();
        } else {
            db.signin(Root {
                username: username.expect("Set SURREAL_TOKEN or SURREAL_USER/SURREAL_PASS"),
                password: password.expect("Set SURREAL_TOKEN or SURREAL_USER/SURREAL_PASS"),
            })
            .await
            .unwrap();
        }

        db.use_ns(namespace).use_db(database).await.unwrap();

        info!("Connected to database.");
    } else {
        return Err(anyhow!(
            "Database hasn't been started. Please start the database."
        ));
    }

    Ok(db)
}

#[cfg(feature = "surrealdb")]
fn get_database_address() -> anyhow::Result<String> {
    if cfg!(target_arch = "wasm32") {
        // IndexedDB currently not working--ideal for WASM, but issues present: https://github.com/surrealdb/indxdb/issues/9
        return Ok("indxdb://MyDatabase".to_string());
    }
    if cfg!(feature = "server") {
        return std::env::var("SURREAL_URL").map_err(|_| anyhow!("Set SURREAL_URL"));
    }
    if cfg!(feature = "client") {
        return CLIENT_DATABASE_ADDRESS.get().cloned().ok_or_else(|| anyhow!("Configure the local application database before connecting"));
    }
    Err(anyhow!(
        "Enable the Flux server or client feature to configure a database URL"
    ))
}

