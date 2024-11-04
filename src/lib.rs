#![allow(warnings)]
#![allow(unused)]

#[cfg(not(target_arch = "xtensa"))]
pub mod binding;
#[cfg(not(target_arch = "xtensa"))]
pub mod builder;
#[cfg(not(target_arch = "xtensa"))]
pub mod elements;
#[cfg(not(target_arch = "xtensa"))]
pub mod constants;
#[cfg(not(target_arch = "xtensa"))]
pub mod functions;
#[cfg(not(target_arch = "xtensa"))]
pub mod systems;
#[cfg(not(target_arch = "xtensa"))]
pub mod database;

#[cfg(not(target_arch = "xtensa"))]
pub mod prelude {
	pub use crate::binding::*;
	pub use crate::builder::*;
	pub use crate::elements::*;
	pub use crate::constants::*;
	pub use crate::functions::*;
	pub use crate::systems::*;
	pub use crate::database::*;
}
