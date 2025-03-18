#[cfg(feature = "bevy")]
use bevy::prelude::*;
use bevy::reflect::{DynamicStruct, DynamicTyped, GetTypeRegistration, TypeRegistration, Typed};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use smart_clone::SmartClone;
use uuid::Uuid;
use anyhow::{Result, anyhow};
use std::fmt::Display;
use crate::prelude::*;
use std::fmt::Debug;

pub mod dynamic_struct_serde;

pub trait FluxRecord = Component + Reflect + PartialReflect + Typed + Clone + Debug + Reactive + GetTypeRegistration + Serialize + DeserializeOwned;

/*
#[cfg(feature = "bevy")]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[derive(Clone, PartialEq, ::prost::Message, Hash, Eq)]
pub struct Thing {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String
}

#[cfg(not(feature = "bevy"))]
#[derive(Clone, PartialEq, ::prost::Message, Hash, Eq)]
pub struct Thing {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String
}
*/

//trait DynamicReflect: PartialReflect + FromReflect {}
//pub type Dynamic = Box<dyn DynamicReflect>;

/*
struct Dynamic(Box<dyn Reflect>);

impl Clone for Dynamic {
    fn clone(&self) -> Self {
        Self(self.0.clone_value())
    }
}

impl PartialReflect for Dynamic {
    fn get_represented_type_info(&self) -> Option<&'static bevy::reflect::TypeInfo> {
        self.0.get_represented_type_info()
    }

    fn into_partial_reflect(self: Box<Self>) -> Box<dyn PartialReflect> {
        self.0.into_partial_reflect()
    }

    fn as_partial_reflect(&self) -> &dyn PartialReflect {
        self.0.as_partial_reflect()
    }

    fn as_partial_reflect_mut(&mut self) -> &mut dyn PartialReflect {
        self.0.as_partial_reflect_mut()
    }

    fn try_into_reflect(self: Box<Self>) -> std::result::Result<Box<dyn Reflect>, Box<dyn PartialReflect>> {
        self.0.try_into_reflect()
    }

    fn try_as_reflect(&self) -> Option<&dyn Reflect> {
        self.0.try_as_reflect()
    }

    fn try_as_reflect_mut(&mut self) -> Option<&mut dyn Reflect> {
        self.0.try_as_reflect_mut()
    }

    fn try_apply(&mut self, value: &dyn PartialReflect) -> std::result::Result<(), bevy::reflect::ApplyError> {
        self.0.try_apply(value)
    }

    fn reflect_ref(&self) -> bevy::reflect::ReflectRef {
        self.0.reflect_ref()
    }

    fn reflect_mut(&mut self) -> bevy::reflect::ReflectMut {
        self.0.reflect_mut()
    }

    fn reflect_owned(self: Box<Self>) -> bevy::reflect::ReflectOwned {
        self.0.reflect_owned()
    }

    fn clone_value(&self) -> Box<dyn PartialReflect> {
        self.0.clone_value()
    }
}

impl TypePath for Dynamic {
    fn type_path() -> &'static str {
        ""
    }

    fn short_type_path() -> &'static str {
        ""
    }
}

impl Typed for Dynamic {
    fn type_info() -> &'static bevy::reflect::TypeInfo {
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

    fn set(&mut self, value: Box<dyn Reflect>) -> std::result::Result<(), Box<dyn Reflect>> {
        todo!()
    }
}

/*
impl GetTypeRegistration for Dynamic {
    fn get_type_registration() -> TypeRegistration {
        todo!()
    }
} */
 */

/// This is a test comment.
#[derive(Reactive, Reflect, Event, SmartClone)]
#[reflect(from_reflect = false)]
//#[derive(ragent::prelude::Task)]
//, serde::Serialize, serde::Deserialize
#[derive(documented::Documented)]
#[allow(clippy::derive_partial_eq_without_eq)]
#[cfg_attr(feature = "prost", derive(::prost::Message))]
pub struct NetworkEvent {
    pub peer_id: Thing,
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    pub ev: DynamicStruct
}

impl NetworkEvent {
    pub fn new<T>(peer_id: Thing, ev: T) -> Self where T: Struct {
        Self {
            peer_id,
            ev: ev.clone_dynamic()
        }
    }

    pub fn get_ev<T>(&self) -> Result<T> where T: FromReflect {
        match T::from_reflect(&self.ev) {
            Some(ev) => Ok(ev),
            None => {
                Err(anyhow!("Failed to cast network event to type!"))
            },
        }
    }
}

#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "prost", derive(::prost::Message))]
#[derive(Clone, PartialEq, Hash, Eq, Default, Debug)] //::prost::Message,
pub struct Thing {
    //#[prost(string, tag = "1")]
    pub id: String
}

impl Thing {
    pub fn new() -> Self {
        return Self::from(&Uuid::new_v4().to_string());
    }

    pub fn from(text: &str) -> Self {
        return Self {
            id: text.replace("-", "")
        }
    }
}

impl Serialize for Thing {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.id)
    }
}

impl<'de> Deserialize<'de> for Thing {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = String::deserialize(deserializer)?;
        Ok(Thing { id })
    }
}

impl std::fmt::Display for Thing {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if f.alternate() {
            // Full UUID when used with "{:#}"
            write!(f, "{}", self.id)
        } else {
            // Short UUID when used with "{}"
            write!(f, "{}", &self.id[..4])
        }
    }
}
