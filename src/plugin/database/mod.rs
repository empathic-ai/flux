mod extensions;
use std::time::Duration;

pub use extensions::*;

use crate::prelude::*;
use bevy::prelude::*;
use surrealdb::{engine::any::Any, opt::auth::Root, *};
use anyhow::{anyhow, Error};
use bevy_wasm_tasks::*;
use bevy_async_ecs::*;

pub fn start(runner: Res<AsyncRunner>, tasks: Tasks, mut config: ResMut<Session>) -> anyhow::Result<()> {
    info!("Starting server...");

    #[cfg(feature = "production")] {
        info!("Starting database...");

        Command::new("rm")
        .args(["mount/efs/database/LOCK"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;

        Command::new("surreal")
        .args(["start", "file://mount/efs/database", "--log", "error", "--no-banner", "--user", "root", "--pass", "root", "--bind", "0.0.0.0:7777"])
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
        let db: Surreal<Any> = Surreal::init();

        if let Ok(_) = db.connect(get_database_address()).await {
            // Signin as a namespace, database, or root user
            #[cfg(feature = "server")]
            db.signin(Root {
                username: "root",
                password: "root",
            })
            .await.unwrap();

            info!("B");
            db.use_ns("test").use_db("test").await.unwrap();

            info!("C");
        } else {
            info!("Failed to connect to database.");
            //return Err(anyhow!("Database hasn't been started. Please start the database."));
        }

        async_world.register_system(move |mut commands: Commands, mut state: ResMut<NextState<DbState>>| {
            commands.insert_resource(DBConfig {
                db: db.clone(),
                id_mappings: Default::default()
            });
            state.set(DbState::Connected);
        }).await.run().await;
    });

    //bevy::tasks::block_on(fut);

    //AsyncComputeTaskPool::get().spawn_local(fut).detach();
    //common::utils::spawn(async move {
        //*done_clone.lock().unwrap() = true;
        //Ok(())
    //});

    Ok(())
}

fn get_database_address<'a>() -> &'a str {
    #[cfg(target_arch = "wasm32")]
    return "indxdb://MyDatabase";
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(feature = "server")]
    return "ws://localhost:7777";
}