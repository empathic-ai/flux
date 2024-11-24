#![allow(warnings)]
#![allow(unused)]
#![feature(let_chains)]

pub mod service {
    tonic::include_proto!("flux");
}

pub use service::*;

#[cfg(feature = "bevy")]
pub mod binding;
#[cfg(feature = "bevy")]
pub mod builder;
#[cfg(feature = "bevy")]
pub mod elements;
#[cfg(feature = "bevy")]
pub mod constants;
#[cfg(feature = "bevy")]
pub mod functions;
#[cfg(feature = "bevy")]
pub mod plugin;
pub mod types;
//#[cfg(not(target_arch = "xtensa"))]
//pub mod dynamic;

pub mod prelude {
	#[cfg(feature = "bevy")]
	pub use crate::binding::*;
	#[cfg(feature = "bevy")]
	pub use crate::builder::*;
	#[cfg(feature = "bevy")]
	pub use crate::elements::*;
	#[cfg(feature = "bevy")]
	pub use crate::constants::*;
	#[cfg(feature = "bevy")]
	pub use crate::functions::*;
	#[cfg(feature = "bevy")]
	pub use crate::plugin::*;
	pub use crate::types::*;
	//pub use crate::dynamic::*;
}


