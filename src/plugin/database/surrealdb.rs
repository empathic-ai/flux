use crate::prelude::*;
use anyhow::{Error, anyhow};
use bevy::prelude::*;
use bevy_async_ecs::*;
use bevy_wasm_tasks::*;
use common::prelude::*;
use futures::lock::Mutex;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use surrealdb::{Surreal, engine::any::Any, opt::auth::Root, types::SurrealValue};

#[derive(Debug, SurrealValue, Serialize, Deserialize)]
pub struct Record {
    #[allow(dead_code)]
    id: surrealdb::types::record_id::RecordId
}

pub fn start(config: Res<FluxConfig>, runner: Res<AsyncRunner>, tasks: Tasks) -> Result {
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

    let api_url: String = config.get_client_api_url();

    tasks.spawn_auto(async move |x| {
        async_world
            .insert_resource(Session::new(get_peer_id(api_url).await))
            .await;

        let db = get_database().await.expect("Failed to connect to database");

        async_world
            .register_system(
                move |mut commands: Commands, mut state: ResMut<NextState<DbState>>| {
                    commands.insert_resource(DBConfig {
                        db: Arc::new(Mutex::new(db.clone())),
                        id_mappings: Default::default(),
                        entity_mappings: Default::default(),
                    });
                    state.set(DbState::Connected);
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

// TODO: Rework to suport dual mode. Cannot be dependent on cfg features
#[cfg(feature = "client")]
async fn get_peer_id(api_url: String) -> Id {
    let peer_id = match is_session(api_url.clone()).await {
        Ok(client_id) => {
            if client_id.is_empty() {
                register(api_url).await.unwrap()
            } else {
                client_id
            }
        }
        Err(err) => {
            info!("Error grabbing session: {}", err);
            register(api_url).await.unwrap()
        }
    };

    info!("Got client ID: {}", peer_id);
    Id::from(&peer_id)
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
        return Err(anyhow!("Database hasn't been started. Please start the database."));
    }

    Ok(db)
}

#[cfg(feature = "server")]
async fn get_peer_id(api_url: String) -> Id {
    Id::nil()
}

#[cfg(feature = "client")]
pub async fn is_session(api_url: String) -> reqwest::Result<String> {
    let client = reqwest::Client::new();
    let mut req = client.post(format!("{}/session", api_url));

    #[cfg(target_arch = "wasm32")]
    {
        req = req.fetch_credentials_include();
    }

    req.send().await?.error_for_status()?.text().await
}

#[cfg(feature = "client")]
pub async fn register(api_url: String) -> reqwest::Result<String> {
    let client = reqwest::Client::new();

    let mut req = client.post(format!("{}/register", api_url));

    #[cfg(target_arch = "wasm32")]
    {
        req = req.fetch_credentials_include();
    }

    req.send().await?.text().await
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

#[cfg(not(any(feature = "client", feature = "server")))]
async fn get_peer_id(_: String) -> Id {
    Id::nil()
}
