#![allow(warnings)]
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {

    let mut builder = tonic_build::configure();

    let mut config = prost_build::Config::new();

    config.extern_path(".flux.Thing", "crate::prelude::Thing");
    //config.extern_path(".flux.Dynamic", "crate::prelude::Dynamic");

    if let Ok(_) = env::var("CARGO_FEATURE_BEVY") {
        builder = builder.type_attribute(".", "#[derive(crate::prelude::Reactive, bevy::prelude::Reflect, bevy::prelude::Event)]");
    }

    
    builder.type_attribute(".", "#[derive(documented::Documented, serde::Serialize, serde::Deserialize)]").compile_with_config(config, &["proto/flux.proto"], &["proto"])?;

    Ok(())
}