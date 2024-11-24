pub mod systems;
pub use systems::*;

use crate::prelude::*;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future::Future;
use std::marker::PhantomData;
use std::option::IterMut;
use std::pin::Pin;

use bevy_async_ecs::*;
use bevy::{ecs::system::SystemParam, prelude::*};
use bevy::ecs::component::Tick;
use bevy::reflect::Typed;

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use surrealdb::{engine::any::Any, Surreal};

use uuid::Uuid;
use anyhow::Result;
use std::fmt::Display;
use std::fmt::Debug;

#[derive(Component, Debug, Default)]
pub struct DBRecord {
    pub id: Thing
}

// TODO: Create better mechanism for indicating a component is loading
#[derive(Component, Debug, Default)]
pub struct Loading {
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

#[derive(Resource)]
pub struct DBConfig {
    #[cfg(not(target_arch = "xtensa"))]
    pub db: Surreal<Any>,
    pub async_world: AsyncWorld,
    pub id_mappings: HashMap<Thing, Entity>
}

impl DBConfig {
    pub fn get_entity(&self, id: &Thing) -> Option<&Entity> {
        self.id_mappings.get(id)
    }

    pub fn insert_entity(&mut self, id: &Thing, entity: Entity) -> Entity{
        self.id_mappings.insert(id.clone(), entity);
        entity
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
    //fn add_or_set_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing>;
    //fn add_or_get_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing>;//Mut<'_, T>;
    fn add_or_set(&mut self, id: Thing, record: T) -> Thing;
    fn add_or_get(&mut self, id: Thing, record: T) -> Mut<'_, T>;
    fn get(&mut self, id: Thing) -> Option<Mut<'_, T>>;
    fn iter(&mut self) -> Vec<(Thing, T)>;
}

impl<'w, 's, T: Component + Reflect + Typed + Serialize + DeserializeOwned + Clone + Debug> DB<T> for DbQuery<'w, 's, T> {

    /*
    fn add_or_set_async(&mut self, id: Thing, record: T) -> Pin<Box<dyn Future<Output = Thing>>> {
        if let Some((mut _record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == id) {
            _record.set(Box::new(record));
            return Box::pin(async move { id });
            //record
        } else {
            let db = &self.db.db;
            let contains = self.cache.cached_records.contains_key(&id.clone());
            
            return Box::pin(async move {
                id
            });
            /*
            let mut o = self.cache.cached_records.entry(id.clone()).or_insert_with(|| {
                if let Some(record) = get_record::<T>(&db, id.clone()).unwrap() {
                    (Tick::new(0), Tick::new(0), record)
                } else {
                    (Tick::new(0), Tick::new(0), record)
                }
            });
            Mut::new(&mut o.2, &mut o.0, &mut o.1, Tick::new(0), Tick::new(0))
            */
        }
    }*/

    /*
    fn add_or_get_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing> { //-> Mut<'_, T> {
        let async_world = self.db.async_world;

        let fut = async move {
            let result = async_world.register_io_system(|query: Query<(&T)>| {
                return id;
            }).await.run(()).await.unwrap();
        };

        /*

        */
        fut
    }
    */

    fn add_or_set(&mut self, id: Thing, record: T) -> Thing {
        //let _ = self.add_or_get(id.clone(), record);
        //id

        if let Some((mut _record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == id) {
            info!("Set record!");
            _record.set(Box::new(record));
        } else {
            info!("Cached record!");
            self.cache.cached_records.entry(id.clone()).insert_entry((Tick::new(0), Tick::new(0), record));
        }
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

