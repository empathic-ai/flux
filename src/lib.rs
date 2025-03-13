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

pub mod binding;
pub mod builder;
pub mod elements;
pub mod constants;
pub mod functions;
pub mod plugin;
pub mod types;

pub mod prelude {
	pub use flux_derive::*;
	pub use flux_core::prelude::*;
	pub use crate::binding::*;
	pub use crate::builder::*;
	pub use crate::elements::*;
	pub use crate::constants::*;
	pub use crate::functions::*;
	pub use crate::plugin::*;
	pub use crate::types::*;
}


