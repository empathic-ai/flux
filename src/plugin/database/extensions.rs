use crate::prelude::*;
use bevy_trait_query::RegisterExt;
use common::prelude::*;
#[cfg(feature = "surrealdb")]
use surrealdb::types::SurrealValue;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future::Future;
use std::marker::PhantomData;
use std::option::IterMut;
use std::panic::Location;
use std::pin::Pin;
use std::sync::Arc;

use bevy::ecs::component::{Mutable, Tick};
use bevy::reflect::{GetTypeRegistration, Typed};
use bevy::{ecs::system::SystemParam, prelude::*};
#[cfg(feature = "surrealdb")]
use bevy_async_ecs::*;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
#[cfg(feature = "surrealdb")]
use surrealdb::{Surreal, engine::any::Any};

use anyhow::Result;
use std::fmt::Debug;
use std::fmt::Display;
use uuid::Uuid;

#[derive(Component, Debug, Default)]
pub struct DBRecord {
    pub id: Id,
}

// TODO: Create better mechanism for indicating a component is loading
#[derive(Component, Debug, Default)]
pub struct Loading {}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct TypedID<T> {
    pub id: String,
    #[reflect(ignore)]
    pub p: PhantomData<T>,
}

impl<T> TypedID<T> {}

#[derive(Resource)]
pub struct DBConfig {
    #[cfg(feature = "surrealdb")]
    pub db: Arc<Surreal<Any>>,
    //pub async_world: AsyncWorld,
    pub id_mappings: HashMap<Id, Entity>,
    pub entity_mappings: HashMap<Entity, Id>,
}

impl DBConfig {
    pub fn get_entity(&self, id: &Id) -> Option<Entity> {
        self.id_mappings.get(id).copied()
    }

    pub fn get_id(&self, entity: Entity) -> Id {
        self.entity_mappings
            .get(&entity)
            .cloned()
            .expect(&format!("Failed to get ID for entity: {entity}"))
    }

    pub fn insert_entity(&mut self, id: &Id, entity: Entity) -> Entity {
        if let Some(previous_entity) = self.id_mappings.insert(*id, entity) {
            if previous_entity != entity {
                self.entity_mappings.remove(&previous_entity);
            }
        }
        if let Some(previous_id) = self.entity_mappings.insert(entity, *id) {
            if previous_id != *id {
                self.id_mappings.remove(&previous_id);
            }
        }
        entity
    }
}

#[derive(Resource)]
pub struct DBCache<T> {
    pub cached_records: HashMap<Id, (Tick, Tick, T)>,
}

impl<T> Default for DBCache<T> {
    fn default() -> Self {
        Self {
            cached_records: Default::default(),
        }
    }
}

#[cfg(feature = "surrealdb")]
#[derive(Clone, Copy, Debug)]
pub struct RecordWriteTiming {
    pub snapshot_window: std::time::Duration,
    pub retry_delay: std::time::Duration,
    pub max_starts_per_update: usize,
    pub max_in_flight: usize,
}

#[cfg(feature = "surrealdb")]
impl Default for RecordWriteTiming {
    fn default() -> Self {
        Self {
            snapshot_window: std::time::Duration::ZERO,
            retry_delay: std::time::Duration::from_secs(5),
            max_starts_per_update: 64,
            max_in_flight: 64,
        }
    }
}

#[cfg(feature = "surrealdb")]
#[derive(Resource, Default)]
pub struct RecordWriteTimings(HashMap<std::any::TypeId, RecordWriteTiming>);

#[cfg(feature = "surrealdb")]
impl RecordWriteTimings {
    pub fn set<T: FluxRecord>(&mut self, timing: RecordWriteTiming) {
        self.0.insert(std::any::TypeId::of::<T>(), timing);
    }

    pub fn get<T: FluxRecord>(&self) -> RecordWriteTiming {
        self.0.get(&std::any::TypeId::of::<T>()).copied().unwrap_or_default()
    }
}

#[cfg(feature = "surrealdb")]
struct PendingWrite<T> {
    pending: Option<T>,
    in_flight: bool,
    retry_at: Option<bevy::platform::time::Instant>,
    dirty_since: Option<bevy::platform::time::Instant>,
}

#[cfg(feature = "surrealdb")]
#[derive(Resource)]
struct RecordWrites<T: Send + Sync + 'static> {
    records: HashMap<Id, PendingWrite<T>>,
    timing: RecordWriteTiming,
}

#[cfg(feature = "surrealdb")]
impl<T: Send + Sync + 'static> Default for RecordWrites<T> {
    fn default() -> Self {
        Self { records: HashMap::new(), timing: RecordWriteTiming::default() }
    }
}

#[cfg(feature = "surrealdb")]
impl<T: Clone + Send + Sync + 'static> RecordWrites<T> {
    fn enqueue(&mut self, id: Id, record: T) {
        self.enqueue_at(id, record, bevy::platform::time::Instant::now());
    }

    fn enqueue_at(&mut self, id: Id, record: T, now: bevy::platform::time::Instant) {
        let state = self.records.entry(id).or_insert(PendingWrite {
            pending: None,
            in_flight: false,
            retry_at: None,
            dirty_since: None,
        });
        state.dirty_since.get_or_insert(now);
        state.pending = Some(record);
    }

    fn ready(&mut self, now: bevy::platform::time::Instant) -> Vec<(Id, T)> {
        let in_flight = self.records.values().filter(|state| state.in_flight).count();
        let capacity = self.timing.max_in_flight.saturating_sub(in_flight)
            .min(self.timing.max_starts_per_update);
        self.records.iter_mut().filter_map(|(id, state)| {
            if state.in_flight || state.retry_at.is_some_and(|deadline| now < deadline) {
                return None;
            }
            if state.dirty_since.is_some_and(|since| now.saturating_duration_since(since) < self.timing.snapshot_window) {
                return None;
            }
            let record = state.pending.take()?;
            state.in_flight = true;
            state.dirty_since = None;
            Some((*id, record))
        }).take(capacity).collect()
    }

    fn complete(&mut self, id: Id, record: T, succeeded: bool, now: bevy::platform::time::Instant) {
        let Some(state) = self.records.get_mut(&id) else { return };
        state.in_flight = false;
        if succeeded {
            state.retry_at = None;
            if state.pending.is_none() {
                self.records.remove(&id);
            }
        } else {
            state.pending.get_or_insert(record);
            state.dirty_since.get_or_insert(now);
            state.retry_at = Some(now + self.timing.retry_delay);
        }
    }
}

#[cfg(all(test, feature = "surrealdb"))]
mod record_write_tests {
    use super::*;
    use bevy::platform::time::Instant;
    use std::time::Duration;

    #[test]
    fn continuous_changes_do_not_postpone_snapshot() {
        let mut writes = RecordWrites::default();
        writes.timing.snapshot_window = Duration::from_secs(1);
        let id = Id::new();
        let now = Instant::now();
        for update in 0..100 {
            let tick = now + Duration::from_millis(update * 10);
            writes.enqueue_at(id, update, tick);
            assert!(writes.ready(tick).is_empty());
        }
        assert_eq!(writes.ready(now + Duration::from_secs(1)), vec![(id, 99)]);
    }

    #[test]
    fn dispatch_limit_retains_undispatched_records() {
        let mut writes = RecordWrites::default();
        writes.timing.max_starts_per_update = 1;
        let now = Instant::now();
        writes.enqueue_at(Id::new(), 1, now);
        writes.enqueue_at(Id::new(), 2, now);
        assert_eq!(writes.ready(now).len(), 1);
        assert_eq!(writes.ready(now).len(), 1);
        assert!(writes.ready(now).is_empty());
    }

    #[test]
    fn concurrent_limit_waits_for_an_acknowledgement() {
        let mut writes = RecordWrites::default();
        writes.timing.max_in_flight = 1;
        let now = Instant::now();
        writes.enqueue_at(Id::new(), 1, now);
        writes.enqueue_at(Id::new(), 2, now);
        let first = writes.ready(now);
        assert_eq!(first.len(), 1);
        assert!(writes.ready(now).is_empty());
        writes.complete(first[0].0, first[0].1, true, now);
        assert_eq!(writes.ready(now).len(), 1);
    }

    #[test]
    fn acknowledgement_keeps_newer_changes_pending() {
        let mut writes = RecordWrites::default();
        let id = Id::new();
        let now = Instant::now();
        writes.enqueue(id, 1);
        assert_eq!(writes.ready(now), vec![(id, 1)]);
        writes.enqueue(id, 2);
        writes.enqueue(id, 3);
        assert!(writes.ready(now).is_empty());
        writes.complete(id, 1, true, now);
        assert_eq!(writes.ready(now), vec![(id, 3)]);
        writes.complete(id, 3, true, now);
        assert!(writes.records.is_empty());
    }

    #[test]
    fn failure_retries_latest_value_after_backoff() {
        let mut writes = RecordWrites::default();
        let id = Id::new();
        let now = Instant::now();
        writes.enqueue(id, 1);
        writes.ready(now);
        writes.enqueue(id, 2);
        writes.complete(id, 1, false, now);
        assert!(writes.ready(now + Duration::from_secs(4)).is_empty());
        assert_eq!(writes.ready(now + Duration::from_secs(5)), vec![(id, 2)]);
    }

    #[test]
    fn failure_without_new_changes_retains_original_value() {
        let mut writes = RecordWrites::default();
        let id = Id::new();
        let now = Instant::now();
        writes.enqueue(id, 1);
        writes.ready(now);
        writes.complete(id, 1, false, now);
        assert_eq!(writes.ready(now + Duration::from_secs(5)), vec![(id, 1)]);
    }
}

#[derive(SystemParam)]
pub struct DbQuery<'w, 's, T: FluxRecord> {
    records_query: Query<'w, 's, (&'static mut T, &'static DBRecord)>,
    cache: ResMut<'w, DBCache<T>>,
    db: Res<'w, DBConfig>,
}

pub trait DB<T> {
    //fn add_or_set_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing>;
    //fn add_or_get_async(&mut self, id: Thing, record: T) -> impl Future<Output = Thing>;//Mut<'_, T>;
    fn add_or_set(&mut self, id: Id, record: T) -> Id;
    fn add_or_get(&mut self, id: Id, record: T) -> &mut T;
    fn get(&mut self, id: &Id) -> Option<Mut<'_, T>>;
    fn iter<'a>(&'a mut self) -> impl Iterator<Item = (Id, Mut<'a, T>)> + 'a
    where
        T: 'a;
}

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
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut db_record = self.add_or_get(id.clone(), record.clone());
            db_record.set(Box::new(record));
        }

        // Needed on web--can't immediateliy send to db
        #[cfg(target_arch = "wasm32")]
        if let Some((mut _record, _)) = self
            .records_query
            .iter_mut()
            .find(|(_, db_record)| db_record.id == id)
        {
            //info!("Set record!");
            _record.set(Box::new(record));
        } else {
            //info!("Cached record!");
            self.cache.cached_records.entry(id.clone()).insert_entry((
                Tick::new(0),
                Tick::new(0),
                record,
            ));
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
        if let Some((mut record, _)) = self
            .records_query
            .iter_mut()
            .find(|(_, db_record)| db_record.id == *id)
        {
            Some(record)
        } else {
            None
        }
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

    fn iter<'a>(&'a mut self) -> impl Iterator<Item = (Id, Mut<'a, T>)> + 'a
    where
        T: 'a,
    {
        self.records_query.iter_mut().map(|mut o| (o.1.id, o.0))

        /*
        let db = &self.db.db;
        //info!("Getting database records, blocking...");
        let records = bevy_block_on(get_records::<T>(&db)).expect(&format!(
            "Failed to get records for {}",
            T::short_type_path()
        ));
        info!(
            "Records found for {}: {}",
            T::short_type_path(),
            records.len()
        );
        records
        */
    }
}

#[cfg(feature = "surrealdb")]
pub async fn upsert_record<T: FluxRecord>(
    db: &Surreal<Any>,
    id: Id,
    record: T,
) -> anyhow::Result<()> {
    use surrealdb::types::SerdeWrapper;

    let o: Option<SerdeWrapper<T>> = db
        .upsert((T::short_type_path(), id.to_pretty_string()))
        .content(SerdeWrapper(record))
        .await?;
    Ok(())
}

#[cfg(feature = "surrealdb")]
pub async fn get_record<T: FluxRecord>(db: &Surreal<Any>, id: Id) -> anyhow::Result<Option<T>> {
    use surrealdb::types::SerdeWrapper;

    let o: Option<SerdeWrapper<T>> = db
        .select((T::short_type_path(), id.to_pretty_string()))
        .await?;
    Ok(o.map(|wrapper| wrapper.0))
}

#[cfg(feature = "surrealdb")]
#[derive(Debug, Serialize, Deserialize)]
pub struct TypedRecord<T>
where
    T: Serialize,
{
    pub id: surrealdb::types::RecordId,
    #[serde(flatten)]
    pub record: T,
}

pub trait FluxRegisterExt {
    fn add_record<T: FluxRecord>(&mut self) -> &mut Self;
    fn add_record_with_policy<T: FluxRecord>(&mut self, policy: RecordPolicy) -> &mut Self;
    #[cfg(feature = "surrealdb")]
    fn add_record_with_timings<T: FluxRecord>(&mut self, timing: RecordWriteTiming) -> &mut Self;
    fn add_reactive<T: FluxRecord>(&mut self) -> &mut Self;
}

impl FluxRegisterExt for App {
    fn add_record<T: FluxRecord>(&mut self) -> &mut Self {
        self.init_resource::<RecordPolicies>()
            .init_resource::<AuthenticatedRecordPeers>()
            .insert_resource(DBCache::<T>::default())
            .add_reactive::<T>();
        //.add_systems(PostStartup, detect_db_changes::<T>)

        #[cfg(feature = "bevy_std")]
        self.add_systems(Update, handle_db_events::<T>.run_if(run_if_db))
            .add_systems(PostUpdate, detect_db_changes::<T>.after(BindingGraphSet).run_if(run_if_db));
        #[cfg(feature = "surrealdb")]
        self.init_resource::<RecordWrites<T>>()
            .init_resource::<RecordWriteTimings>();
        //.add_systems(Update, handle_db_events::<T>.before(detect_db_changes::<T>))

        #[cfg(all(feature = "server", feature = "bevy_std"))]
        self.add_systems(PostUpdate, replicate_owned_records::<T>.after(BindingGraphSet).run_if(run_if_db));

        self
    }

    fn add_record_with_policy<T: FluxRecord>(&mut self, policy: RecordPolicy) -> &mut Self {
        self.add_record::<T>();
        self.world_mut()
            .resource_mut::<RecordPolicies>()
            .set::<T>(policy);
        self
    }

    #[cfg(feature = "surrealdb")]
    fn add_record_with_timings<T: FluxRecord>(&mut self, timing: RecordWriteTiming) -> &mut Self {
        self.add_record::<T>();
        self.world_mut()
            .resource_mut::<RecordWriteTimings>()
            .set::<T>(timing);
        self
    }

    fn add_reactive<T: FluxRecord>(&mut self) -> &mut Self {
        self.register_component_as::<dyn Reactive, T>()
            .register_type::<T>()
    }
}

fn run_if_db(res: Option<Res<DBConfig>>, state: Option<Res<State<DatabaseState>>>) -> bool {
    res.is_some() && state.is_some_and(|state| *state.get() == DatabaseState::Ready)
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

#[cfg(feature = "bevy_std")]
fn handle_db_events<T: FluxRecord>(
    mut commands: Commands,
    mut db_request_evs: EventReader<DbRequestEvent>,
    mut db_receive_evs: EventReader<DbReceiveEvent>,
    policies: Res<RecordPolicies>,
    peers: Res<AuthenticatedRecordPeers>,
) {
    let policy = policies.get::<T>();
    for ev in db_request_evs.read() {
        if ev.component_type.as_deref().is_some_and(|name| name != T::short_type_path()) {
            continue;
        }
        let id = ev.db_record_id;
        let peer_id = ev.peer_id;
        if !policy.can_read(id, peers.principal(peer_id)) {
            if ev.component_type.is_some() {
                warn!(record_id = %id, component_type = T::short_type_path(), peer_id = %peer_id, "Record component request denied by read policy");
            }
            continue;
        }
        commands.try_get_record(
            id,
            move |record: InOption<T>,
                  config: Res<Session>,
                  peers: Res<AuthenticatedRecordPeers>,
                  policies: Res<RecordPolicies>| {
                // Recheck at send time: a login may have changed while the DB read was pending.
                if let Some(record) = record.get() {
                    if !policies.can_read_record(id, peers.principal(peer_id), record) {
                        warn!(record_id = %id, component_type = T::short_type_path(), peer_id = %peer_id, "Record component request denied by record authorization");
                        return;
                    }
                    info!(
                        "Sending {:#}.{} to peer {:#}.",
                        id,
                        T::short_type_path().to_string(),
                        peer_id
                    );

                    config.get_multiplexer().send_ev(
                        Id::nil(),
                        peer_id,
                        AddComponentEvent {
                            entity_id: Some(id),
                            component_type: T::short_type_path().to_string(),
                            component: record.to_dynamic_struct(),
                        },
                    );
                }
            },
        );
    }
    for ev in db_receive_evs.read() {
        if ev.component_type != T::short_type_path() {
            continue;
        }
        #[cfg(feature = "server")]
        if !policy.client_writes {
            continue;
        }
        #[cfg(not(feature = "server"))]
        if !policy.client_writes && ev.peer_id != Id::nil() {
            continue;
        }
        if let Some(record) = T::from_dynamic(&ev.component.to_dynamic_struct()) {
            commands.upsert_record(ev.db_record_id, record, |_: InMut<T>| {});
        }
    }
}

#[cfg(feature = "bevy_std")]
fn detect_db_changes<T: FluxRecord>(
    mut commands: Commands,
    mut db_config: ResMut<DBConfig>,
    mut set: Query<(Entity, &T, &DBRecord), (Or<(Added<T>, Changed<T>)>)>,
    mut cache: ResMut<DBCache<T>>,
    policies: Res<RecordPolicies>,
    peers: Res<AuthenticatedRecordPeers>,
    session: Res<Session>,
    #[cfg(feature = "surrealdb")] mut writes: ResMut<RecordWrites<T>>,
    #[cfg(feature = "surrealdb")] timings: Res<RecordWriteTimings>,
) {
    let policy = policies.get::<T>();
    if !policy.automatic_persistence {
        return;
    }
    let type_name = T::short_type_path();

    //info!("Detecting database changes for {}...", type_name);

    #[cfg(feature = "surrealdb")]
    {
        writes.timing = timings.get::<T>();
        for (entity, record, db_record) in set.iter_mut() {
            //changed_ev_writer.send(entity.clone());
            //println!("UPDATING DATABASE");
            //info!("Detected add or change in component. Type: {} ID: {}", type_name, db_record.id);

            writes.enqueue(db_record.id, record.clone());

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
        for (id, record) in writes.ready(bevy::platform::time::Instant::now()) {
            let db = db_config.db.clone();
            commands.run(async move |world| {
                let result = upsert_record(&db, id, record.clone()).await;
                if let Err(error) = &result {
                    tracing::error!(record_type = T::short_type_path(), %id, %error, "Automatic record persistence failed; retaining pending write");
                }
                world.apply(move |world: &mut World| {
                    world.resource_mut::<RecordWrites<T>>().complete(
                        id, record, result.is_ok(), bevy::platform::time::Instant::now(),
                    );
                }).await;
            });
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

#[cfg(feature = "server")]
fn replicate_owned_records<T: FluxRecord>(
    set: Query<(&T, &DBRecord), Changed<T>>,
    policies: Res<RecordPolicies>,
    peers: Res<AuthenticatedRecordPeers>,
    session: Res<Session>,
) {
    let policy = policies.get::<T>();
    if !matches!(policy.read, RecordReadAccess::OwnerByRecordId | RecordReadAccess::Authorized) {
        return;
    }
    for (record, db_record) in &set {
        for peer in peers.authorized_readers(&policies, db_record.id, record) {
            session.get_multiplexer().send_ev(
                Id::nil(),
                peer,
                AddComponentEvent {
                    entity_id: Some(db_record.id),
                    component_type: T::short_type_path().to_string(),
                    component: record.to_dynamic_struct(),
                },
            );
        }
    }
}
