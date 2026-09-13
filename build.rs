fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ordinary source edits do not change the generated protocol.
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "tonic")]
    compile_protocol()?;
    Ok(())
}

#[cfg(feature = "tonic")]
fn compile_protocol() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-env-changed=PROTOC");
    println!("cargo:rerun-if-env-changed=PROTOC_INCLUDE");
    let mut config = prost_build::Config::new();
    config.extern_path(".flux.Thing", "crate::prelude::Thing");
    // Historical alternative: config.extern_path(".flux.Dynamic", "crate::prelude::Dynamic");

    let mut builder = tonic_build::configure();
    if cfg!(feature = "bevy") {
        builder = builder.type_attribute(
            ".",
            "#[derive(crate::prelude::Reactive, bevy::prelude::Reflect, bevy::prelude::Event)]",
        );
    }
    builder
        .type_attribute(
            ".",
            "#[derive(documented::Documented, serde::Serialize, serde::Deserialize)]",
        )
        .compile_with_config(config, &["proto/flux.proto"], &["proto"])?;
    Ok(())
}
