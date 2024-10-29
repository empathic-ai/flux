
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::marker::PhantomData;
use std::option::IterMut;

use bevy::{ecs::system::SystemParam, prelude::*};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use surrealdb::{engine::any::Any, Surreal};

use bevy::ecs::component::Tick;
use bevy::reflect::Typed;
use uuid::Uuid;
use anyhow::Result;
use std::fmt::Display;
use std::fmt::Debug;

#[derive(Component, Debug, Default)]
pub struct DBRecord {
    pub id: Thing
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct TypedID<T> {
    pub id: String,
    #[reflect(ignore)]
    pub p: PhantomData<T>
}

impl<T> TypedID<T> {
    
}

#[derive(Clone, PartialEq, ::prost::Message, Hash, Eq)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
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

#[derive(Resource)]
pub struct DBConfig {
    #[cfg(not(target_arch = "xtensa"))]
    pub db: Surreal<Any>,
    pub id_mappings: HashMap<Thing, Entity>
}

impl DBConfig {
    pub fn get_entity(&self, id: &Thing) -> Option<&Entity> {
        self.id_mappings.get(id)
    }

    pub fn insert_entity(&mut self, id: Thing, entity: Entity) {
        self.id_mappings.insert(id, entity);
    }
}

#[derive(Resource, Default)]
pub struct DBCache<T> {
    pub cached_records: HashMap<Thing, (Tick, Tick, T)>
}

#[derive(SystemParam)]
pub struct DbQuery<'w, 's, T: Component + Reflect + Typed + Serialize + DeserializeOwned + Clone + Debug> {
    records_query: Query<'w, 's, (&'static mut T, &'static DBRecord)>,
    cache: ResMut<'w, DBCache<T>>,
    db: Res<'w, DBConfig>
}

pub trait DB<T> {
    fn add_or_set(&mut self, id: Thing, record: T) -> Thing;
    fn add_or_get(&mut self, id: Thing, record: T) -> Mut<'_, T>;
    fn get(&mut self, id: Thing) -> Option<Mut<'_, T>>;
    fn iter(&mut self) -> Vec<(Thing, T)>;
}

impl<'w, 's, T: Component + Reflect + Typed + Serialize + DeserializeOwned + Clone + Debug> DB<T> for DbQuery<'w, 's, T> {

    fn add_or_set(&mut self, id: Thing, record: T) -> Thing {
        let _ = self.add_or_get(id.clone(), record);
        id
    }

    fn add_or_get(&mut self, id: Thing, record: T) -> Mut<'_, T> {
        if let Some((mut record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == id) {
            record
        } else {
            let db = &self.db.db;
            let mut o = self.cache.cached_records.entry(id.clone()).or_insert_with(|| {
                if let Some(record) = bevy::tasks::block_on(get_record::<T>(&db, id.clone())).unwrap() {
                    (Tick::new(0), Tick::new(0), record)
                } else {
                    (Tick::new(0), Tick::new(0), record)
                }
            });
            Mut::new(&mut o.2, &mut o.0, &mut o.1, Tick::new(0), Tick::new(0))
        }
    }

    // Returns None if the entry doesn't exist as an active record, cached record or db record
    fn get(&mut self, id: Thing) -> Option<Mut<'_, T>> {
        if let Some((mut record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == id) {
            Some(record)
        } else {
            let db = &self.db.db;

            let o = match self.cache.cached_records.entry(id.clone()) {
                Entry::Occupied(o) => Some(o.into_mut()),
                Entry::Vacant(v) => {
                    let mut o: Option<&mut (Tick, Tick, T)> = None;
                    if let Ok(record) = bevy::tasks::block_on(get_record::<T>(&db, id.clone())) {
                        if let Some(record) = record {
                            let mut v = v.insert((Tick::new(0), Tick::new(0), record));
                            o = Some(v);
                        }
                    }
                    o
                }
            };

            if let Some(o) = o {
                Some(Mut::new(&mut o.2, &mut o.0, &mut o.1, Tick::new(0), Tick::new(0)))
            } else {
                None
            }

            /*
            if let Some(record) = record {
                self.config.cached_records.insert(id.clone(), (Tick::new(0), Tick::new(0), Box::new(record)));
                if let Some(mut o) = self.config.cached_records.get_mut(&id.clone()) {
                    if let Some(record) = o.2.downcast_mut::<T>() {
                        Some(Mut::new(record, &mut o.0, &mut o.1, Tick::new(0), Tick::new(0)))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
            */
        }
    }
    
    //fn iter(&mut self) -> Vec<Mut<'_, T>> {
        // Gets cached records--may use later for more real-time updates but currently just grabbing all records from db
        //let mut iter = self.cache.cached_records.iter_mut().map(|mut o| Mut::new(&mut o.1.2, &mut o.1.0, &mut o.1.1, Tick::new(0), Tick::new(0))).collect();
    //}

    fn iter(&mut self) -> Vec<(Thing, T)> {
        let db = &self.db.db;
        let records = bevy::tasks::block_on(get_records::<T>(&db)).expect("Failed to get records.");
        println!("Records found for {}: {}", T::short_type_path(), records.len());
        records
    }
}


pub async fn get_record<T: Typed + DeserializeOwned>(db: &Surreal<Any>, id: Thing) -> anyhow::Result<Option<T>> {
    let o: Option<T> = db.select((T::short_type_path(), id.id.clone())).await?;
    Ok(o)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TypedRecord<T> where T: Debug + Serialize {
    #[allow(dead_code)]
    id: surrealdb::sql::Thing,
    #[serde(flatten)]
    record: T
}

pub async fn get_records<T: Typed + Serialize + DeserializeOwned + Clone + Debug>(db: &Surreal<Any>) -> anyhow::Result<Vec<(Thing, T)>> {
    let o: Vec<TypedRecord<T>> = db.select(T::short_type_path()).await?;
    let o = o.iter().map(|record| {
        (Thing::from(&record.id.id.to_string()), record.record.clone())
    }).collect();
    Ok(o)
}

