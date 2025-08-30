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

/*
impl<T: ?Sized + 'static> SystemInput for InOptionIn<T> {
    type Param<'a> = InOption<'a, T>;
    type Inner<'a> = Option<&'a mut T>;

    fn wrap(this: Self::Inner<'_>) -> Self::Param<'_> {
        match this {
            Some(inner) => InOption::Some(inner),
            None => InOption::None,
        }
    }
}

pub trait InOptionTrait<'a, T: Sized + 'static> : SystemInput<Param<'a> = InOption<'a, T>, Inner<'a> =  Option<&'a mut T>> {
}


impl <'a, T: Sized + 'static> InOptionTrait<'a, T> for InOption<'a, T> {

}

pub struct InOptionParam<'i, T: ?Sized>(pub InOption<'i, T>);

impl<'i, T: ?Sized + 'static> SystemInput for InOptionParam<'i, T> {
    type Param<'j> = InOptionParam<'j, T>;
    type Inner<'j> = Option<&'j mut T>;

    fn wrap(this: Self::Inner<'_>) -> Self::Param<'_> {
        InOptionParam(match this {
            Some(v) => InOption::Some(v),
            None => InOption::None,
        })
    }
}



impl<'i, T> Deref for InOption<'i, T> {
    type Target = Option<&'i T>;

    fn deref(&self) -> &Self::Target {
        if let Self::Some(inner) = self {
            Some(inner)
        } else {
            &None
        }
    }
}

impl<'i, T: ?Sized> DerefMut for InOption<'i, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
} */