#![allow(warnings)]
#![allow(unused)]
#![feature(let_chains)]
#![feature(trait_alias)]

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

pub mod binding;
#[cfg(feature = "bevy_ui")]
pub mod builder;
#[cfg(feature = "bevy_ui")]
pub mod elements;

pub mod constants;
pub mod functions;
pub mod plugin;
pub mod types;

pub mod prelude {
	#[cfg(feature = "server")]
	pub use crate::server::*;
	#[cfg(feature = "client")]
	pub use crate::client::*;
	pub use flux_derive::*;
	pub use flux_core::prelude::*;
	pub use crate::binding::*;
	#[cfg(feature = "bevy_ui")]
	pub use crate::builder::*;
	#[cfg(feature = "bevy_ui")]
	pub use crate::elements::*;
	pub use crate::constants::*;
	pub use crate::functions::*;
	pub use crate::plugin::*;
	pub use crate::types::*;
	pub use reflect_steroids::prelude::*;
}


