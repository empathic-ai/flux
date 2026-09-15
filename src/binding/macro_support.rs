//! Implementation support for flux-derive's checked paths.
use crate::prelude::Id;
use bevy::{prelude::Component, reflect::TypePath};

pub fn component_name<T: Component + TypePath>() -> &'static str {
    T::short_type_path()
}
mod sealed {
    use super::Id;
    pub trait IdReference {}
    impl IdReference for Id {}
    impl IdReference for Option<Id> {}
}
pub fn assert_id<T: sealed::IdReference>(_: &T) {}
