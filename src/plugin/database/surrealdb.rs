use crate::prelude::*;
use anyhow::{Error, anyhow};
use bevy::prelude::*;
use bevy_async_ecs::*;
use bevy_wasm_tasks::*;
use common::prelude::*;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use surrealdb::{Surreal, engine::any::Any, opt::auth::Root, types::SurrealValue};

#[derive(Clone)]
pub struct Database {
    connection: Arc<Surreal<Any>>,
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
            .bind(("first_table", First::short_type_path()))
            .bind(("first_id", first_id.to_pretty_string()))
            .bind(("first", SerdeWrapper(first)))
            .bind(("second_table", Second::short_type_path()))
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

    pub fn from_connection(connection: Arc<Surreal<Any>>) -> Self {
        Self { connection }
    }

    pub async fn upsert_record<T: FluxRecord>(&self, id: Id, record: T) -> anyhow::Result<()> {
        upsert_record(&self.connection, id, record).await
    }

    pub async fn create_record<T: FluxRecord>(&self, id: Id, record: T) -> anyhow::Result<Option<T>> {
        use surrealdb::types::SerdeWrapper;
        let created: Option<SerdeWrapper<T>> = self.connection
            .create((T::short_type_path(), id.to_pretty_string()))
            .content(SerdeWrapper(record)).await?;
        Ok(created.map(|record| record.0))
    }

    pub async fn restrict_record_table<T: FluxRecord>(&self) -> anyhow::Result<()> {
        restrict_record_table::<T>(&self.connection).await
    }

    pub async fn run_migrations(&self, migrations: &[Migration]) -> anyhow::Result<()> {
        run_migrations_on(&self.connection, migrations).await
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

/// Connect to the configured SurrealDB
pub async fn get_database() -> anyhow::Result<Surreal<Any>> {
    let database_address = get_database_address()?;

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
        return Ok("file://database.db".to_string());
    }
    Err(anyhow!(
        "Enable the Flux server or client feature to configure a database URL"
    ))
}

