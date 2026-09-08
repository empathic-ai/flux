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

        let db: Surreal<Any> = Surreal::init();

        info!("Connecting to database.");
        if let Ok(_) = db.connect(get_database_address()).await {
            // Signin as a namespace, database, or root user
            #[cfg(feature = "server")]
            db.signin(Root {
                username: "root".to_string(),
                password: "root".to_string(),
            })
            .await
            .unwrap();

            db.use_ns("test").use_db("test").await.unwrap();

            info!("Connected to database.");
        } else {
            info!("Failed to connect to database.");
            //return Err(anyhow!("Database hasn't been started. Please start the database."));
        }

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
fn get_database_address<'a>() -> &'a str {
    #[cfg(target_arch = "wasm32")]
    // IndexedDB currently not working--ideal for WASM, but issues present: https://github.com/surrealdb/indxdb/issues/9
    return "indxdb://MyDatabase";
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(feature = "client")]
    return "file://database.db";
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(feature = "server")]
    return "ws://localhost:7777";
}
