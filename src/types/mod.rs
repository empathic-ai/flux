#[cfg(feature = "bevy")]
use bevy::prelude::*;
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;
use anyhow::Result;
use std::fmt::Display;

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

#[cfg_attr(feature = "bevy", derive(Reflect))]
#[derive(Clone, PartialEq, ::prost::Message, Hash, Eq)]
pub struct Thing {
    #[prost(string, tag = "1")]
    pub id: ::prost::alloc::string::String
}

impl Thing {
    pub fn new() -> Self {
        return Self { id: Uuid::new_v4().to_string() };
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

impl Display for Thing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.id, f)
    }
}
