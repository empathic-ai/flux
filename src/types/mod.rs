use crate::prelude::*;
#[cfg(feature = "bevy")]
use bevy::{ecs::component::Mutable, prelude::*, asset::ron::ser::{PrettyConfig, to_string_pretty}};
#[cfg(feature = "bevy")]
use bevy_reflect::{GetTypeRegistration, Typed};
#[cfg(feature = "bevy_reflect")]
use bevy_reflect::{DynamicStruct, prelude::*};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
#[cfg(feature = "serde")]
use serde_with::serde_as;
use smart_clone::SmartClone;
use std::{fmt::Debug, str::FromStr};
use uuid::Uuid;

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
pub mod dynamic_list_serde;

#[cfg(feature = "bevy")]
pub trait ToStringPretty {
    fn to_string_pretty(&self) -> String;
}

#[cfg(feature = "bevy")]
impl<T> ToStringPretty for T
where
    T: PartialReflect
{
    fn to_string_pretty(&self) -> String {
        self.as_partial_reflect().to_string_pretty()
    }
}

#[cfg(feature = "bevy")]
impl ToStringPretty for dyn Reactive {
    fn to_string_pretty(&self) -> String {
        self.as_partial_reflect().to_string_pretty()
    }
}

#[cfg(feature = "bevy")]
impl ToStringPretty for dyn PartialReflect {
    fn to_string_pretty(&self) -> String {
        use bevy_reflect::{TypeRegistry, serde::ReflectSerializer};

        let mut registry = TypeRegistry::new();
        registry.register_global_types();

        let serializer = ReflectSerializer::new(self, &registry);
        to_string_pretty(&serializer, PrettyConfig::default())
            .unwrap()
    }
}

#[cfg(feature = "bevy")]
pub trait ToDynamicStruct {
    fn to__dynamic_struct(&self) -> Option<DynamicStruct>;
}

#[cfg(feature = "bevy")]
impl<T> ToDynamicStruct for T
where
    T: PartialReflect
{
    fn to__dynamic_struct(&self) -> Option<DynamicStruct> {
        self.as_partial_reflect().to__dynamic_struct()
    }
}

#[cfg(feature = "bevy")]
impl ToDynamicStruct for dyn PartialReflect {
    fn to__dynamic_struct(&self) -> Option<DynamicStruct> {
        use bevy_reflect::ReflectRef;

        if let ReflectRef::Struct(s) = self.reflect_ref() {
            let mut dynamic = DynamicStruct::default();
            dynamic.set_represented_type(s.get_represented_type_info());
            for i in 0..s.field_len() {
                let name = s.name_at(i).unwrap();
                let field = s.field_at(i).unwrap();
                dynamic.insert_boxed(name, field.clone_value());
            }
            // dynamic: DynamicStruct
            return Some(dynamic);
        }
        None
    }
}


#[cfg(feature = "bevy")]
pub trait FluxRecord = Component<Mutability = Mutable>
    + Struct
    + PartialReflect
    + FromReflect
    + Typed
    + Clone
    + Debug
    + Reactive
    + GetTypeRegistration
    + Serialize
    + DeserializeOwned;

#[cfg_attr(feature = "bevy", derive(Event))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone)]
pub struct PeerEvent {
    pub peer_id: Option<Id>,
    pub network_event: Option<NetworkEvent>,
}

#[cfg_attr(feature = "bevy", derive(States))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum DbState {
    #[default]
    Connecting,
    Connected,
}

#[cfg_attr(feature = "bevy", derive(States))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum NetworkState {
    #[default]
    Connecting,
    Connected,
}

///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct AddEntityEvent {
    pub entity_id: Option<Id>,
}
///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RemoveEntityEvent {}
///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct TrackRecordEvent {
    pub entity_id: Id,
}
///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct UntrackEntityEvent {}

///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RemoveComponentEvent {}
///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct ChangePropertyEvent {}
///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct AddElementEvent {
    pub entity_id: String,
    pub component_type: String,
    pub property_name: String,
    pub index: i32,
    pub value: String,
}
///
#[derive(Reflect, Reactive, documented::Documented)]
#[cfg_attr(feature = "bevy", derive(Component, Event))]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Clone, Default, PartialEq, Debug)]
pub struct RemoveElementEvent {
    pub entity_id: String,
    pub component_type: String,
    pub property_name: String,
    pub index: i32,
}

/// This is a placeholder comment.
#[derive(
    Reactive, Reflect, SmartClone, documented::Documented
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", derive(Event))]
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug)]
pub struct AddComponentEvent {
    pub entity_id: Option<Id>,
    pub component_type: String,
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    #[serde(with = "dynamic_struct_serde")]
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

#[derive(Clone)]
#[cfg_attr(feature = "bevy", derive(Event))]
pub struct DbRequestEvent {
    pub peer_id: Id,
    pub db_record_id: Id,
}

/// This is a test comment.
#[derive(
    Reactive, Reflect, SmartClone, documented::Documented
)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "bevy", derive(Event))]
pub struct DbReceiveEvent {
    pub peer_id: Id,
    pub db_record_id: Id,
    pub component_type: String,
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    #[serde(with = "dynamic_struct_serde")]
    pub component: DynamicStruct,
}

/// This is a test comment.
#[derive(Reactive, Reflect, SmartClone, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Event))]
//#[reflect(from_reflect = false)]
//#[derive(ragent::prelude::Task)]
#[derive(documented::Documented)]
#[allow(clippy::derive_partial_eq_without_eq)]
#[cfg_attr(feature = "prost", derive(::prost::Message))]
pub struct NetworkEvent {
    pub peer_id: Id,
    #[clone(clone_with = "DynamicStruct::clone_dynamic")]
    #[serde(with = "dynamic_struct_serde")]
    pub ev: DynamicStruct,
}

impl NetworkEvent {
    pub fn new<T>(peer_id: Id, ev: T) -> Self
    where
        T: Struct,
    {
        Self {
            peer_id,
            ev: ev.to_dynamic_struct(),
        }
    }

    pub fn get_ev<T>(&self) -> Option<T>
    where
        T: FromDynamic,
    {
        T::from_dynamic(&self.ev)
    }

    pub fn get_ev_name(&self) -> String {
        match self.ev.get_represented_type_info() {
            Some(type_info) => type_info.ty().short_path().to_string(),
            None => "dynamic".to_string(),
        }
    }
}

#[cfg_attr(feature = "bevy_reflect", derive(Reflect))]
#[derive(Clone, Copy, PartialEq, Hash, Eq, Default, Debug, Reactive)]
pub struct Id {
    id: Uuid,
}

impl Id {
    pub fn new() -> Self {
        return Self { id: Uuid::new_v4() };
    }

    pub fn nil() -> Self {
        return Self { id: Uuid::nil() };
    }

    pub fn from(text: &str) -> Self {
        Self {
            id: Uuid::from_str(text).unwrap(),
        }
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
