#![allow(warnings)]
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {

    let mut config = prost_build::Config::new();
    config.extern_path(".flux.Thing", "crate::prelude::Thing");
    //config.extern_path(".flux.Dynamic", "crate::prelude::Dynamic");

    let attribute = "#[derive(documented::Documented, serde::Serialize, serde::Deserialize)]";
    if let Ok(_) = env::var("CARGO_FEATURE_TONIC") {
        let mut builder = tonic_build::configure();

        if let Ok(_) = env::var("CARGO_FEATURE_BEVY") {
            builder = builder.type_attribute(".", "#[derive(crate::prelude::Reactive, bevy::prelude::Reflect, bevy::prelude::Event)]");
        }

        builder.type_attribute(".", attribute).compile_with_config(config, &["proto/flux.proto"], &["proto"])?;
    } else {
        config.type_attribute(".", attribute).compile_protos(&["proto/flux.proto"], &["proto"]);
    }

    Ok(())
}