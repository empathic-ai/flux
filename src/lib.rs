#![allow(warnings)]
#![feature(let_chains)]
#![feature(trait_alias)]
#![feature(string_remove_matches)]
#![feature(must_not_suspend)]
#![warn(must_not_suspend)]

// Re-export the SDK used by the public database helpers so callers share its types.
#[cfg(feature = "surrealdb")]
pub use ::surrealdb as surrealdb_client;

#[cfg(feature = "tonic")]
pub mod service {
	use crate::prelude::*;
	
	include!(concat!(env!("OUT_DIR"), concat!("/", "flux.rs")));
    //tonic::include_proto!("flux");
}
#[cfg(feature = "tonic")]
pub use service::*;

#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "bevy")]
pub mod binding;
#[cfg(feature = "bevy_std")]
pub mod builder;
#[cfg(feature = "bevy_std")]
pub mod elements;

#[cfg(feature = "bevy")]
pub mod constants;
#[cfg(feature = "bevy")]
pub mod functions;
#[cfg(feature = "bevy")]
pub mod plugin;
#[cfg(feature = "bevy_reflect")]
pub mod types;

pub mod prelude {
	#[cfg(feature = "server")]
	pub use crate::server::*;
	#[cfg(feature = "client")]
	pub use crate::client::*;
	#[cfg(feature = "bevy_reflect")]
	pub use flux_derive::*;
	#[cfg(feature = "bevy_reflect")]
	pub use flux_core::prelude::*;
	#[cfg(feature = "bevy")]
	pub use crate::binding::*;
	#[cfg(feature = "bevy_std")]
	pub use crate::builder::*;
	#[cfg(feature = "bevy_std")]
	pub use crate::elements::*;
	#[cfg(feature = "bevy")]
	pub use crate::constants::*;
	#[cfg(feature = "bevy")]
	pub use crate::functions::*;
	#[cfg(feature = "bevy")]
	pub use crate::plugin::*;
	#[cfg(feature = "bevy_reflect")]
	pub use crate::types::*;
	#[cfg(feature = "bevy_reflect")]
	pub use reflect_steroids::prelude::*;
}


