use bevy::{prelude::*, reflect::{impl_type_path, DynamicStruct, DynamicTyped, DynamicVariant, GetType, GetTypeRegistration, MaybeTyped, Type, TypeRegistration, Typed}};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use crate::prelude::*;
use smart_clone::SmartClone;

#[bevy_trait_query::queryable]
#[reflect_trait]
pub trait Bindable {
    fn get(&self) -> Box<dyn Reflect>;
    fn set(&mut self, value: Box<dyn Reflect>);
}

#[derive(Component)]
pub struct AutoBindable {
    pub value: Box<dyn Reflect>
}

#[derive(Component, SmartClone, Reflect, Reactive, Serialize, Deserialize, Debug)]
#[reflect(from_reflect = false)]
pub struct ReactiveView {
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    #[serde(with = "dynamic_struct_serde")]
    pub value: DynamicStruct
}

#[derive(Debug, Reactive)]
pub struct Dynamic {
    value: Box<dyn PartialReflect>
    //Struct(DynamicStruct),
    //Opaque(Box<dyn PartialReflect>)
}

impl DynamicTyped for Dynamic {
    fn reflect_type_info(&self) -> &'static bevy::reflect::TypeInfo {
        todo!()
    }
}

impl Dynamic {
    pub fn new<T: PartialReflect>(value: &T) -> Self {
        Self {
            value: value.clone_value()
        }
    }
}

impl MaybeTyped for Dynamic {
    fn maybe_type_info() -> Option<&'static bevy::reflect::TypeInfo> {
        None
    }
}

impl Clone for Dynamic {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone_value()
        }
        /*
        match self {
            Self::Struct(arg0) => Self::Struct(arg0.clone_dynamic()),
            Self::Opaque(arg0) => Self::Opaque(arg0.clone_value()),
        }
        */
    }
}

impl_type_path!((in flux) Dynamic);

/*
impl TypePath for Dynamic {
    fn type_path() -> &'static str {
        "flux::prelude::dynamic"
    }

    fn short_type_path() -> &'static str {
        "dynamic"
    }
} */

impl GetTypeRegistration for Dynamic {
    fn get_type_registration() -> bevy::reflect::TypeRegistration {
        TypeRegistration::of::<()>()
    }
}

impl FromReflect for Dynamic {
    fn from_reflect(reflect: &dyn PartialReflect) -> Option<Self> {
        todo!()
    }
}

impl Reflect for Dynamic {
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        todo!()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        todo!()
    }

    fn into_reflect(self: Box<Self>) -> Box<dyn Reflect> {
        todo!()
    }

    fn as_reflect(&self) -> &dyn Reflect {
        todo!()
    }

    fn as_reflect_mut(&mut self) -> &mut dyn Reflect {
        todo!()
    }

    fn set(&mut self, value: Box<dyn Reflect>) -> Result<(), Box<dyn Reflect>> {
        todo!()
    }
}

/*
impl Struct for Dynamic {
    fn field(&self, name: &str) -> Option<&dyn PartialReflect> {
        todo!()
    }

    fn field_mut(&mut self, name: &str) -> Option<&mut dyn PartialReflect> {
        todo!()
    }

    fn field_at(&self, index: usize) -> Option<&dyn PartialReflect> {
        todo!()
    }

    fn field_at_mut(&mut self, index: usize) -> Option<&mut dyn PartialReflect> {
        todo!()
    }

    fn name_at(&self, index: usize) -> Option<&str> {
        todo!()
    }

    fn field_len(&self) -> usize {
        todo!()
    }

    fn iter_fields(&self) -> bevy::reflect::FieldIter {
        todo!()
    }

    fn clone_dynamic(&self) -> DynamicStruct {
        todo!()
    }
}
*/

impl PartialReflect for Dynamic {
    fn get_represented_type_info(&self) -> Option<&'static bevy::reflect::TypeInfo> {
        self.value.get_represented_type_info()
    }

    fn into_partial_reflect(self: Box<Self>) -> Box<dyn PartialReflect> {
        self.value.into_partial_reflect()
    }

    fn as_partial_reflect(&self) -> &dyn PartialReflect {
        self.value.as_partial_reflect()
    }

    fn as_partial_reflect_mut(&mut self) -> &mut dyn PartialReflect {
        self.value.as_partial_reflect_mut()
    }

    fn try_into_reflect(self: Box<Self>) -> Result<Box<dyn Reflect>, Box<dyn PartialReflect>> {
        self.value.try_into_reflect()
    }

    fn try_as_reflect(&self) -> Option<&dyn Reflect> {
        self.value.try_as_reflect()
    }

    fn try_as_reflect_mut(&mut self) -> Option<&mut dyn Reflect> {
        self.value.try_as_reflect_mut()
    }

    fn try_apply(&mut self, value: &dyn PartialReflect) -> Result<(), bevy::reflect::ApplyError> {
        self.value.try_apply(value)
    }

    fn reflect_ref(&self) -> bevy::reflect::ReflectRef {
        self.value.reflect_ref()
        //bevy::reflect::ReflectRef::Struct(self)
    }

    fn reflect_mut(&mut self) -> bevy::reflect::ReflectMut {
        self.value.reflect_mut()
    }

    fn reflect_owned(self: Box<Self>) -> bevy::reflect::ReflectOwned {
        self.value.reflect_owned()
    }

    fn clone_value(&self) -> Box<dyn PartialReflect> {
        self.value.clone_value()
    }
}

impl Serialize for Dynamic {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer {
        todo!()
    }
}

impl<'de> Deserialize<'de> for Dynamic {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de> {
        todo!()
    }
}
