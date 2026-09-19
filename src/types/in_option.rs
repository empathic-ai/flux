use bevy::{ecs::{component::Mutable, system::SystemParam}, prelude::*, reflect::Typed};
use serde::de::DeserializeOwned;
use std::{marker::PhantomData, ops::{Deref, DerefMut}};
use crate::prelude::*;

pub struct InOptionIn<T: ?Sized + 'static>(PhantomData<fn(&mut T)>);

#[derive(Debug)]
pub enum InOption<'a, T: ?Sized> {
    Some(&'a mut T),
    None,
}

impl<'a, T: ?Sized> InOption<'a, T> {
    pub fn get(self) -> Option<&'a mut T> {
        match self {
            InOption::Some(inner) => Some(inner),
            InOption::None => None,
        }
    }
}

impl<'a, T: ?Sized + 'static> SystemInput for InOption<'a, T> {
    type Param<'i> = InOption<'i, T>;
    type Inner<'i> = Option<&'i mut T>;

    fn wrap(this: Self::Inner<'_>) -> Self::Param<'_> {
        match this {
            Some(inner) => InOption::Some(inner),
            None => InOption::None,
        }
    }
}