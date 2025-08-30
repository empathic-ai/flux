use bevy_trait_query::RegisterExt;
use common::prelude::*;
#[cfg(feature = "surrealdb")]
use futures::lock::Mutex;
use crate::prelude::*;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future::Future;
use std::marker::PhantomData;
use std::option::IterMut;
use std::panic::Location;
use std::pin::Pin;
#[cfg(feature = "surrealdb")]
use std::sync::Arc;

#[cfg(feature = "surrealdb")]
use bevy_async_ecs::*;
use bevy::{ecs::system::SystemParam, prelude::*};
use bevy::ecs::component::{Mutable, Tick};
use bevy::reflect::{GetTypeRegistration, Typed};

use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "surrealdb")]
use surrealdb::{engine::any::Any, Surreal};

use uuid::Uuid;
use anyhow::Result;
use std::fmt::Display;
use std::fmt::Debug;

#[derive(Component, Debug, Default)]
pub struct DBRecord {
    pub id: Id
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
    #[cfg(feature = "surrealdb")]
    pub db: Arc<Mutex<Surreal<Any>>>,
    //pub async_world: AsyncWorld,
    pub id_mappings: HashMap<Id, Entity>
}


impl DBConfig {
    pub fn get_entity(&self, id: &Id) -> Option<&Entity> {
        self.id_mappings.get(id)
    }

    pub fn insert_entity(&mut self, id: &Id, entity: Entity) -> Entity{
        self.id_mappings.insert(id.clone(), entity);
        entity
    }
}

#[derive(Resource)]
pub struct DBCache<T> {
    pub cached_records: HashMap<Id, (Tick, Tick, T)>
}

impl<T> Default for DBCache<T> {
    fn default() -> Self {
        Self { cached_records: Default::default() }
    }
}

/*
#[derive(SystemParam)]
pub struct DbQuery<'w, 's, T: FluxRecord> {
    records_query: Query<'w, 's, (&'static mut T, &'static DBRecord)>,
    cache: ResMut<'w, DBCache<T>>,
    db: Res<'w, DBConfig>
}


#[cfg(feature = "surrealdb")]
pub trait DB<T> {
    //fn add_or_set_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing>;
    //fn add_or_get_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing>;//Mut<'_, T>;
    fn add_or_set(&mut self, id: Id, record: T) -> Id;
    fn add_or_get(&mut self, id: Id, record: T) ->  &mut T;
    fn get(&mut self, id: &Id) -> Option<Mut<'_, T>>;
    fn iter(&mut self) -> Vec<(Id, T)>;
}

#[cfg(feature = "surrealdb")]
impl<'w, 's, T: FluxRecord> DB<T> for DbQuery<'w, 's, T> {

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

    // TODO: Create shared setup between WASM and other platforms
    // Probably rewrite DB processes signficantly (using bevy async?)
    fn add_or_set(&mut self, id: Id, record: T) -> Id {
        // Works outside of web--sends immediately to db
        #[cfg(not(target_arch = "wasm32"))] {
            let mut db_record = self.add_or_get(id.clone(), record.clone());
            db_record.set(Box::new(record));
        }

        // Needed on web--can't immediateliy send to db
        #[cfg(target_arch = "wasm32")]
        if let Some((mut _record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == id) {
            //info!("Set record!");
            _record.set(Box::new(record));
        } else {
            //info!("Cached record!");
            self.cache.cached_records.entry(id.clone()).insert_entry((Tick::new(0), Tick::new(0), record));
        }
        
        id
    }

    fn add_or_get(&mut self, id: Id, record: T) -> &mut T {

        todo!()
        /*
        if let Some((mut record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == id) {
            &mut record
        } else {
            #[cfg(feature = "surrealdb")]
            let db = &self.db.db;

            let mut o = self.cache.cached_records.entry(id.clone()).or_insert_with(|| {

                #[cfg(feature = "surrealdb")]
                if let Some(record) = bevy_block_on(get_record::<T>(&db, id.clone())).unwrap() {
                    (Tick::new(0), Tick::new(0), record)
                } else {
                    bevy_block_on(upsert_record::<T>(&db, id.clone(), record.clone()));
                    (Tick::new(0), Tick::new(0), record.clone())
                }

                #[cfg(not(feature = "surrealdb"))]
                (Tick::new(0), Tick::new(0), record.clone())
            });
            &mut o.2
            //Mut::new(&mut o.2, &mut o.0, &mut o.1, Tick::new(0), Tick::new(0))
        }
        */
    }

    // Returns None if the entry doesn't exist as an active record, cached record or db record
    fn get(&mut self, id: &Id) -> Option<Mut<'_, T>> {
        todo!()
        /*
        if let Some((mut record, _)) = self.records_query.iter_mut().find(|(_, db_record)| db_record.id == *id) {
            Some(record)
        } else {
            let db = &self.db.db;

            let o = match self.cache.cached_records.entry(id.clone()) {
                Entry::Occupied(o) => Some(o.into_mut()),
                Entry::Vacant(v) => {
                    let mut o: Option<&mut (Tick, Tick, T)> = None;
                    info!("Getting database record, blocking...");
                    if let Ok(record) = bevy_block_on(get_record::<T>(&db, id.clone())) {
                        if let Some(record) = record {
                            let mut v = v.insert((Tick::new(0), Tick::new(0), record));
                            o = Some(v);
                        }
                    }
                    o
                }
            };

            if let Some(o) = o {
                use bevy::ecs::change_detection::MaybeLocation;

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
        */
    }
    
    //fn iter(&mut self) -> Vec<Mut<'_, T>> {
        // Gets cached records--may use later for more real-time updates but currently just grabbing all records from db
        //let mut iter = self.cache.cached_records.iter_mut().map(|mut o| Mut::new(&mut o.1.2, &mut o.1.0, &mut o.1.1, Tick::new(0), Tick::new(0))).collect();
    //}

    fn iter(&mut self) -> Vec<(Id, T)> {
        let db = &self.db.db;
        //info!("Getting database records, blocking...");
        let records = bevy_block_on(get_records::<T>(&db)).expect(&format!("Failed to get records for {}", T::short_type_path()));
        info!("Records found for {}: {}", T::short_type_path(), records.len());
        records
    }
}
*/

#[cfg(feature = "surrealdb")]
pub async fn upsert_record<T: Typed + Serialize + DeserializeOwned>(db: &Surreal<Any>, id: Id, record: T) -> anyhow::Result<()> {
    let o: Option<T> = db.upsert((T::short_type_path(), id.to_pretty_string())).content(record).await?;
    Ok(())
}

#[cfg(feature = "surrealdb")]
pub async fn get_record<T: Typed + DeserializeOwned>(db: &Surreal<Any>, id: Id) -> anyhow::Result<Option<T>> {
    let o: Option<T> = db.select((T::short_type_path(), id.to_pretty_string())).await?;
    Ok(o)
}

#[cfg(feature = "surrealdb")]
#[derive(Debug, Serialize, Deserialize)]
pub struct TypedRecord<T> where T: Debug + Serialize {
    #[allow(dead_code)]
    id: surrealdb::sql::Thing,
    #[serde(flatten)]
    record: T
}

#[cfg(feature = "surrealdb")]
pub async fn get_records<T: Typed + Serialize + DeserializeOwned + Clone + Debug>(db: &Surreal<Any>) -> anyhow::Result<Vec<(Id, T)>> {
    let o: Vec<TypedRecord<T>> = db.select(T::short_type_path()).await?;
    let o = o.iter().map(|record| {
        (Id::from(&record.id.id.to_string()), record.record.clone())
    }).collect();
    Ok(o)
}


pub trait FluxRegisterExt {
    fn add_record<T: FluxRecord>(&mut self) -> &mut Self;
    fn add_reactive<T: FluxRecord>(&mut self) -> &mut Self;
}


impl FluxRegisterExt for App {
    fn add_record<T: FluxRecord>(&mut self) -> &mut Self {
        self.insert_resource(DBCache::<T>::default())
            .add_reactive::<T>();
            //.add_systems(PostStartup, detect_db_changes::<T>)

        #[cfg(feature = "bevy_std")]
        self.add_systems(PreUpdate, detect_db_changes::<T>.run_if(run_if_db))
            .add_systems(Update, (handle_db_events::<T>, detect_db_changes::<T>).chain().run_if(run_if_db))
            .add_systems(PostUpdate, detect_db_changes::<T>.run_if(run_if_db));
            //.add_systems(Update, handle_db_events::<T>.before(detect_db_changes::<T>))
        
        self
    }
    
    fn add_reactive<T: FluxRecord>(&mut self) -> &mut Self {
        self.register_component_as::<dyn Reactive, T>()
            .add_systems(PreStartup, register_reactive_type::<T>)
    }
}

fn run_if_db(res: Option<Res<DBConfig>>) -> bool
{
    res.is_some()
}

/*
fn on_add_component<T: Component<Mutability = Mutable> + Struct + Reflect + PartialReflect + Typed + Clone + Debug + Reactive + GetTypeRegistration + Serialize + DeserializeOwned>(
    trigger: Trigger<OnAdd, T>,
    mut commands: Commands,
    query: Query<(Entity, &T, &DBRecord)>
) {
    let entity = trigger.target();
    let (_, record, db_record) = query.get(entity).unwrap();

    commands.run(async |world| {
        world.upsert_record(id, record)
    });
    /*
    commands.run(async move |world| {
        info!("Upserting space record.");
        let frog_space_id = world.upsert_record(
            Id::from("fd503b86-c422-4170-9877-35648bed6821"),
            Space {
                ..Default::default()
            },
        ).await;
    }); */
}
*/

fn register_reactive_type<T: FluxRecord>(
    mut config: ResMut<BindingsConfig>
) {
    config.type_registry.register::<T>();
}

#[cfg(feature = "bevy_std")]
fn handle_db_events<T: FluxRecord>(
    mut config: ResMut<Session>,
    mut commands: Commands,
    mut db_request_evs: EventReader<DbRequestEvent>,
    mut db_receive_evs: EventReader<DbReceiveEvent>
) {

    //info!("Handling database changes for {}...", T::short_type_path());

    for ev in db_request_evs.read() {
        let id = ev.db_record_id;
        let peer_id = ev.peer_id;

        commands.get_record(id, move |record: InOption::<T>, mut config: ResMut<Session>| {
            if let Some(record) = record.get() {
                config.get_multiplexer().send_ev(
                    Id::nil(),
                    peer_id,
                    AddComponentEvent {
                        entity_id: Some(id),
                        component_type: T::short_type_path().to_string(), //component.name().to_string(),
                        // TODO: Rewrite once intellisense is working, wrong value here
                        component: record.clone_dynamic()
                    }
                );
            }
        });
    }
    for ev in db_receive_evs.read() {
        if ev.component_type == T::short_type_path() {
            // TODO: Uncomment once intellisense is working
            //query.add_or_set(ev.db_record_id.clone(), T::decode(ev.component_data.as_slice()).expect("Failed to decode record!"));
        }
    }
}

fn detect_db_changes<T: FluxRecord>(
    mut commands: Commands,
    mut db_config: ResMut<DBConfig>,
    mut set: Query<(Entity, &T, &DBRecord), (Or<(Added<T>, Changed<T>)>)>,
    mut cache: ResMut<DBCache<T>>
) {
    let type_name = T::short_type_path();

    //info!("Detecting database changes for {}...", type_name);

    #[cfg(feature = "surrealdb")] {
        for (entity, record, db_record) in set.iter_mut() {
            //changed_ev_writer.send(entity.clone());
            //println!("UPDATING DATABASE");
            //info!("Detected add or change in component. Type: {}", type_name);

            let db = db_config.db.clone();
            let record = record.clone();
            let id = db_record.id;

            commands.run(async move |world| {
                world.upsert_record(id, record).await;
            });
            
            /*
            #[cfg(not(target_arch = "wasm32"))]
            bevy_block_on(async move {
                let c: Option<Record> = db
                    .update((type_name.to_string(), id))
                    .content(record)
                    .await
                    .unwrap();
            });
       
            let id = db_record.id.clone().id;
            let record = record.clone();
            bevy::tasks::block_on(async {
                let c: Option<Record> = db_config
                    .db
                    .update(surrealdb::sql::Thing::from((
                        type_name.to_string(),
                        surrealdb::sql::Id::String(id),
                    )))
                    .content(record)
                    .await
                    .unwrap();
            });
            */
        }
    }
        /*
    for (id, (_, _, record)) in cache.cached_records.iter() {
        //info!("Spawning cached record. Type: {}", type_name);

        if let Some(entity) = db_config.get_entity(&id) {
            //info!("Found existing entity. Adding component. Type: {}", type_name);
            commands.entity(*entity).insert(record.clone());
        } else {
            let entity = commands.spawn((DBRecord { id: id.clone() }, record.clone())).id();
            db_config.insert_entity(&id, entity);
        }
        
        #[cfg(not(target_arch = "wasm32"))]
        #[cfg(feature = "surrealdb")] {
            let db = db_config.db.clone();
            let record = record.clone();
            let id = id.to_string();
    
            bevy_block_on(async move {
                //info!("Adding record to database!");
                let c: Option<Record> = db
                .update((type_name.to_string(), id))
                .content(record)
                .await
                .unwrap();
            });
        }
        

        let id = id.clone().id;
        bevy::tasks::block_on(async {
            let c: Option<Record> = db_config
                .db
                .update(surrealdb::sql::Thing::from((
                    type_name.to_string(),
                    surrealdb::sql::Id::String(id),
                )))
                .content(record)
                .await
                .unwrap();
        });
    }

    cache.cached_records.clear();
    */
}