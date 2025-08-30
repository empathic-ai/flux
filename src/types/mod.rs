#[cfg(feature = "bevy")]
use bevy::prelude::*;
use bevy::{ecs::component::Mutable, reflect::{DynamicStruct, DynamicTyped, GetTypeRegistration, TypeRegistration, Typed}};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use smart_clone::SmartClone;
use uuid::Uuid;
use crate::prelude::*;
use std::{fmt::Debug, str::FromStr};

mod config;
pub use config::*;

#[cfg(feature = "futures")]
mod async_runner;
#[cfg(feature = "futures")]
pub use async_runner::*;

#[cfg(feature = "futures")]
mod in_option;
#[cfg(feature = "futures")]
pub use in_option::*;

pub mod dynamic_struct_serde;

pub trait FluxRecord = Component<Mutability = Mutable> + Struct + Reflect + PartialReflect + Typed + Clone + Debug + Reactive + GetTypeRegistration + Serialize + DeserializeOwned;

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


/// This is a placeholder comment.
#[derive(Reflect, Event, SmartClone)]
#[derive(documented::Documented, serde::Serialize, serde::Deserialize)]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug)]
pub struct AddComponentEvent {
    pub entity_id: Option<Id>,
    pub component_type: String,
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    #[serde(with = "dynamic_struct_serde")]
    #[reflect(ignore)]
    pub component: DynamicStruct,
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
#[derive(Clone, Copy, PartialEq, Hash, Eq, Default, Debug, Reactive)]
pub struct Id {
    id: Uuid
}

impl Id {
    pub fn new() -> Self {
        return Self { id: Uuid::new_v4() };
    }

    pub fn nil() -> Self {
        return Self { id: Uuid::nil() };
    }

    pub fn from(text: &str) -> Self {
        Self { id: Uuid::from_str(text).unwrap() }
    }

    pub fn to_pretty_string(&self) -> String {
        let mut pretty_string = self.id.to_string();
        pretty_string.remove_matches("-");
        pretty_string
    }
}

impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.id.to_string())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = String::deserialize(deserializer)?;
        Ok(Id::from(&id))
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if f.alternate() {
            // Short UUID when used with "{:#}"
            write!(f, "{}", &self.to_pretty_string()[..4])
        } else {
            // Full UUID when used with "{}"
            let mut pretty_string = self.id.to_string();
            pretty_string.remove_matches("-");
        
            write!(f, "{}", pretty_string)
        }
    }
}
