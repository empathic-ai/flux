#[cfg(feature = "bevy")]
use bevy::prelude::*;
use bevy::{ecs::component::Mutable, reflect::{DynamicStruct, DynamicTyped, GetTypeRegistration, TypeRegistration, Typed}};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use smart_clone::SmartClone;
use uuid::Uuid;
use anyhow::{Result, anyhow};
use std::fmt::Display;
use crate::prelude::*;
use std::fmt::Debug;

#[cfg(feature = "futures")]
mod async_runner;
#[cfg(feature = "futures")]
use async_runner::*;

pub mod dynamic_struct_serde;
//pub mod dynamic_variant_serde;

pub trait FluxRecord = Component<Mutability = Mutable> + Reflect + PartialReflect + Typed + Clone + Debug + Reactive + GetTypeRegistration + Serialize + DeserializeOwned;

#[derive(Event)]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone)]
pub struct PeerEvent {
    pub peer_id: Option<Id>,
    pub network_event: Option<NetworkEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, States)]
pub enum DbState {
	#[default]
	Connecting,
    Connected
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, States)]
pub enum NetworkState {
	#[default]
	Connecting,
    Connected
}

#[derive(Resource, Clone)]
pub struct FluxConfig {
    api_url: String
}

impl FluxConfig {
    pub fn new(api_url: String) -> Self {
        Self {
            api_url
        }
    }

    pub fn get_api_url(&self) -> String {
        self.api_url.clone()
    }
}

/*
#[cfg(feature = "bevy")]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[derive(Clone, PartialEq, ::prost::Message, Hash, Eq)]
pub struct Thing {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String
}

#[cfg(not(feature = "db"))]
#[derive(Clone, PartialEq, Hash, Eq)]
pub struct Thing {
    //#[prost(string, tag = "1")]
    pub id: String
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

#[derive(Event, Clone)]
pub struct DbRequestEvent {
    pub peer_id: Id,
    pub db_record_id: Id,
}

#[derive(Event, Clone)]
pub struct DbReceiveEvent {
    pub peer_id: Id,
    pub db_record_id: Id,
    pub component_type: String,
    pub component_data: Vec<u8>
}

/// This is a test comment.
#[derive(Reactive, Reflect, Event, SmartClone, Serialize, Deserialize)]
#[reflect(from_reflect = false)]
//#[derive(ragent::prelude::Task)]
#[derive(documented::Documented)]
#[allow(clippy::derive_partial_eq_without_eq)]
#[cfg_attr(feature = "prost", derive(::prost::Message))]
pub struct NetworkEvent {
    pub peer_id: Id,
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    #[serde(with = "dynamic_struct_serde")]
    pub ev: DynamicStruct
}

impl NetworkEvent {
    pub fn new<T>(peer_id: Id, ev: T) -> Self where T: Struct {
        Self {
            peer_id,
            ev: ev.clone_dynamic()
        }
    }

    pub fn get_ev<T>(&self) -> Option<T> where T: FromDynamic {
        T::from_dynamic(&self.ev)
    }

    pub fn get_ev_name(&self) -> String {
        match self.ev.get_represented_type_info() {
            Some(type_info) => {
                type_info.ty().short_path().to_string()
            },
            None => {
                "dynamic".to_string()
            },
        }
    }
}

#[cfg_attr(feature = "bevy", derive(Reflect))]
#[cfg_attr(feature = "prost", derive(::prost::Message))]
#[derive(Clone, PartialEq, Hash, Eq, Default, Debug, Reactive)] //::prost::Message,
pub struct Id {
    //#[prost(string, tag = "1")]
    pub id: String
}

impl Id {
    pub fn new() -> Self {
        return Self::from(&Uuid::new_v4().to_string());
    }

    pub fn nil() -> Self {
        return Self::from(&Uuid::nil().to_string());
    }

    pub fn from(text: &str) -> Self {
        return Self {
            id: text.replace("-", "")
        }
    }
}

impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.id)
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = String::deserialize(deserializer)?;
        Ok(Id { id })
    }
}

impl std::fmt::Display for Id {
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
