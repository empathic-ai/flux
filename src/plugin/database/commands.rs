use std::marker::PhantomData;
#[cfg(feature = "surrealdb")]
use std::sync::Arc;

use crate::prelude::*;
use bevy::{
    ecs::{
        component::Mutable,
        system::{
            ExclusiveSystemParamFunction, ParamBuilder, RunSystemOnce, SystemParam, SystemState,
        },
    },
    prelude::*,
    reflect::Typed,
};
use bevy_async_ecs::AsyncWorld;
use bevy_wasm_tasks::Tasks;
#[cfg(feature = "surrealdb")]
use futures::lock::Mutex;
use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "surrealdb")]
use surrealdb::types::{RecordId, SerdeWrapper};
#[cfg(feature = "surrealdb")]
use surrealdb::{Surreal, engine::any::Any};

trait UpsertSys<T, SM> = SystemParamFunction<SM> + 'static
where
    T: FluxRecord,
    SM: Send + Sync + 'static,
    // enforce that S’s output is `()`
    <Self as SystemParamFunction<SM>>::Out: std::convert::Into<()>,
    // for *every* lifetime 'a, S::In must be InMut<'a, T>
    for<'a> <Self as SystemParamFunction<SM>>::In: SystemInput<Inner<'a> = &'a mut T>,
    // S’s Param must be a SystemParam and be 'static
    <Self as SystemParamFunction<SM>>::Param: SystemParam + 'static;

trait GetRecordSys<T, O, SM> = SystemParamFunction<SM, Out = O> + 'static
where
    T: FluxRecord,
    SM: Send + Sync + 'static,
    // enforce that S’s output is `()`
    //<Self as SystemParamFunction<SM>>::Out: std::convert::Into<()>,
    // for *every* lifetime 'a, S::In must be InMut<'a, T>
    for<'a> <Self as SystemParamFunction<SM>>::In: SystemInput<Inner<'a> = &'a mut T>,
    // S’s Param must be a SystemParam and be 'static
    <Self as SystemParamFunction<SM>>::Param: SystemParam + 'static;

trait TryGetRecordSys<T, O, SM> = SystemParamFunction<SM, Out = O> + 'static
where
    T: FluxRecord,
    SM: Send + Sync + 'static,
    // enforce that S’s output is `()`
    //<Self as SystemParamFunction<SM>>::Out: std::convert::Into<()>,
    // for *every* lifetime 'a, S::In must be InMut<'a, T>
    for<'a> <Self as SystemParamFunction<SM>>::In: SystemInput<Inner<'a> = Option<&'a mut T>>,
    // S’s Param must be a SystemParam and be 'static
    <Self as SystemParamFunction<SM>>::Param: SystemParam + 'static;

trait GetRecordsSys<T, O, SM> = SystemParamFunction<SM, Out = O> + 'static
where
    T: FluxRecord,
    SM: Send + Sync + 'static,
    // enforce that S’s output is `()`
    //<Self as SystemParamFunction<SM>>::Out: std::convert::Into<()>,
    // for *every* lifetime 'a, S::In must be InMut<'a, T>
    for<'a> <Self as SystemParamFunction<SM>>::In: SystemInput<Inner<'a> = Vec<(Id, T)>>,
    // S’s Param must be a SystemParam and be 'static
    <Self as SystemParamFunction<SM>>::Param: SystemParam + 'static;

pub struct UpsertRecordWithCallback<T, S, SM>
where
    S: UpsertSys<T, SM>,
{
    id: Id,
    record: T,
    system: S,
    smarker_data: PhantomData<SM>,
}

impl<T, S, SM> UpsertRecordWithCallback<T, S, SM>
where
    S: UpsertSys<T, SM>,
{
    fn new(id: Id, record: T, system: S) -> Self {
        Self {
            id,
            record,
            system,
            smarker_data: PhantomData,
        }
    }
}

impl<T, S, SM> Command for UpsertRecordWithCallback<T, S, SM>
where
    S: UpsertSys<T, SM>,
{
    fn apply(mut self, world: &mut World) {
        let found = {
            let mut system_state: SystemState<Query<(Entity, &DBRecord)>> = SystemState::new(world);
            let query = system_state.get_mut(world);
            query
                .iter()
                .find(|(_, db_rec)| db_rec.id == self.id)
                .map(|(entity, _)| entity)
        };

        let Some(entity) = found else {
            spawn_record_with_callback(world, self.id, self.record, self.system);
            return;
        };

        world.entity_mut(entity).insert(self.record);

        let mut system_state: SystemState<(Query<&mut T>, S::Param)> = SystemState::new(world);
        let (mut query, params) = system_state.get_mut(world);
        let mut record = query.get_mut(entity).expect("component was just inserted");

        self.system.run(&mut record, params);
        system_state.apply(world);
    }
}

pub trait AsyncDbCommandsExt {
    async fn upsert_record<T>(&self, id: Id, record: T) -> Id
    where
        T: FluxRecord;

    async fn upsert_record_with_callback<T, S, SM>(&self, id: Id, record: T, system: S)
    where
        S: UpsertSys<T, SM>;

    async fn get_record<T, O, S, SM>(&self, id: Id, system: S)
    where
        S: GetRecordSys<T, O, SM>;

    async fn try_get_record<T, O, S, SM>(&self, id: Id, system: S)
    where
        S: TryGetRecordSys<T, O, SM>;

    async fn get_records<T, O, S, SM>(&self, system: S)
    where
        S: GetRecordsSys<T, O, SM>;
}

impl AsyncDbCommandsExt for AsyncWorld {
    async fn upsert_record<T>(&self, id: Id, record: T) -> Id
    where
        T: FluxRecord,
    {
        
        let type_name = T::short_type_path();

        let mut is_record = false;

                
        #[cfg(feature = "surrealdb")] {
            let (output_tx, output_rx) = async_channel::bounded(1);

            let _record = record.clone();

            self.apply(move |world: &mut World| {
                let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, Tasks, Res<DBConfig>)> = SystemState::new(world);
                {
                    let (mut query, tasks, db_config) = system_state.get_mut(world);
                        
                    let db = db_config.db.clone();

                    tasks.spawn_auto(async move |_| {

                        let mut db = db.lock().await;
                        let result: Option<SerdeWrapper<T>> = db
                            .upsert((type_name.to_string(), id.to_string()))
                            .content(SerdeWrapper(_record))
                            .await
                            .unwrap();
                        //info!("Added record of type {} to database! ID: {}", type_name, id);
                        output_tx.send(()).await;
                    });
                }
                system_state.apply(world);
            }).await;

            output_rx.recv().await;
        }

        self.apply(move |world: &mut World| {
            let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, Tasks, Res<DBConfig>)> = SystemState::new(world);
            {
                {
                    let (mut query, tasks, db_config) = system_state.get_mut(world);
                    
                    if let Some((mut _record, _)) =
                        query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
                    {
                        if _record
                            .reflect_partial_eq(record.as_partial_reflect())
                            .is_none_or(|x| !x)
                        {
                            _record.apply(record.as_partial_reflect());
                        }
                        is_record = true;
                    }
                }
                system_state.apply(world);
            }

            if !is_record {
                spawn_record(world, id, record);
            }
        }).await;

        //self.apply(UpsertRecord::new(id, record)).await;
        id
    }

    async fn upsert_record_with_callback<T, S, SM>(&self, id: Id, record: T, mut system: S)
    where
        S: UpsertSys<T, SM>,
    {
        self.apply(UpsertRecordWithCallback::new(id, record, system))
            .await;
    }

    async fn get_record<T, O, S, SM>(&self, id: Id, mut system: S)
    where
        S: GetRecordSys<T, O, SM>,
    {
        #[cfg(feature = "surrealdb")]
        {
            let (output_tx, output_rx) = async_channel::bounded(1);
            let async_world = self.clone();

            self.apply(move |world: &mut World| {
                let mut system_state: SystemState<(Tasks, Res<DBConfig>)> = SystemState::new(world);
                let (tasks, db_config) = system_state.get_mut(world);
                let db = db_config.db.clone();

                tasks.spawn_auto(async move |_| {
                    get_record(async_world.clone(), db, id, system).await;
                    output_tx.send(()).await;
                });
            })
            .await;

            output_rx.recv().await;
        }
    }

    /// Applies the given `Command` to the world.
    async fn try_get_record<T, O, S, SM>(&self, id: Id, mut system: S)
    where
        S: TryGetRecordSys<T, O, SM>,
    {
        #[cfg(feature = "surrealdb")]
        {
            let (output_tx, output_rx) = async_channel::bounded(1);
            let async_world = self.clone();

            self.apply(move |world: &mut World| {
                let mut system_state: SystemState<(Tasks, Res<DBConfig>)> = SystemState::new(world);
                let (tasks, db_config) = system_state.get_mut(world);
                let db = db_config.db.clone();

                tasks.spawn_auto(async move |_| {
                    try_get_record(async_world.clone(), db, id, system).await;
                    output_tx.send(()).await;
                });
            })
            .await;

            output_rx.recv().await;
        }
    }

    async fn get_records<T, O, S, SM>(&self, mut system: S)
    where
        S: GetRecordsSys<T, O, SM>,
    {
        #[cfg(feature = "surrealdb")]
        {
            let (output_tx, output_rx) = async_channel::bounded(1);
            let async_world = self.clone();

            self.apply(move |world: &mut World| {
                let mut system_state: SystemState<(Tasks, Res<DBConfig>)> = SystemState::new(world);
                let (tasks, db_config) = system_state.get_mut(world);
                let db = db_config.db.clone();

                tasks.spawn_auto(async move |_| {
                    let records = get_records::<T>(db).await.expect("Failed to get records");

                    async_world
                        .apply(move |world: &mut World| {
                            let mut system_state: SystemState<(S::Param)> = SystemState::new(world);
                            let (params) = system_state.get_mut(world);
                            system.run(records, params);
                            system_state.apply(world);
                        })
                        .await;

                    output_tx.send(()).await;
                });
            })
            .await;

            output_rx.recv().await.unwrap();
        }
    }
}

#[cfg(feature = "surrealdb")]
async fn upsert_record<T, O, S, SM>(
    async_world: AsyncWorld,
    db: Arc<Mutex<Surreal<Any>>>,
    id: Id,
    mut system: S,
) where
    S: UpsertSys<T, SM>,
{
    let record: Option<SerdeWrapper<T>> = db
        .lock()
        .await
        .select((T::short_type_path(), id.to_pretty_string()))
        .await
        .unwrap();
    if let Some(mut record) = record {
        let record = record.0;

        async_world
            .apply(move |world: &mut World| {
                let is_record = {
                    let mut system_state: SystemState<Query<(Entity, Mut<T>, &DBRecord)>> =
                        SystemState::new(world);
                    let query = system_state.get_mut(world);
                    query
                        .iter()
                        .find(|(_, _, db_rec)| db_rec.id == id).is_some()
                };

                if !is_record {
                    spawn_record(world, id, record);
                }

                let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> =
                    SystemState::new(world);
                {
                    let (mut query, params) = system_state.get_mut(world);

                    if let Some((mut record, _)) =
                        query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
                    {
                        let record = record.as_mut();
                        system.run(record, params);
                    }
                }
                system_state.apply(world);
            })
            .await;
    }
}

#[cfg(feature = "surrealdb")]
async fn get_record<T, O, S, SM>(
    async_world: AsyncWorld,
    db: Arc<Mutex<Surreal<Any>>>,
    id: Id,
    mut system: S,
) where
    S: GetRecordSys<T, O, SM>,
{
    let record: Option<SerdeWrapper<T>> = db
        .lock()
        .await
        .select((T::short_type_path(), id.to_pretty_string()))
        .await
        .unwrap();
    if let Some(mut record) = record {
        let record = record.0;

        async_world
            .apply(move |world: &mut World| {
                let is_record = {
                    let mut system_state: SystemState<Query<(Entity, Mut<T>, &DBRecord)>> =
                        SystemState::new(world);
                    let query = system_state.get_mut(world);
                    query
                        .iter()
                        .any(|(_, _, db_rec)| db_rec.id == id)
                };

                if !is_record {
                    spawn_record(world, id, record);
                }

                let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> =
                    SystemState::new(world);
                {
                    let (mut query, params) = system_state.get_mut(world);

                    if let Some((mut record, _)) =
                        query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
                    {
                        let record = record.as_mut();
                        system.run(record, params);
                    }
                }
                system_state.apply(world);
            })
            .await;
    }
}

#[cfg(feature = "surrealdb")]
async fn try_get_record<T, O, S, SM>(
    async_world: AsyncWorld,
    db: Arc<Mutex<Surreal<Any>>>,
    id: Id,
    mut system: S,
) where
    S: TryGetRecordSys<T, O, SM>,
{
    let record: Result<Option<SerdeWrapper<T>>, _> = db
        .lock()
        .await
        .select((T::short_type_path(), id.to_pretty_string()))
        .await;

    if let Ok(Some(mut record)) = record {
        let record = record.0;
        async_world
            .apply(move |world: &mut World| {
                let found = {
                    let mut system_state: SystemState<Query<(Entity, &DBRecord)>> =
                        SystemState::new(world);
                    let query = system_state.get_mut(world);
                    query
                        .iter()
                        .find(|(_, db_rec)| db_rec.id == id)
                        .map(|(entity, _)| entity)
                };

                if found.is_none() {
                    spawn_record(world, id, record);
                }

                let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> =
                    SystemState::new(world);
                {
                    let (mut query, params) = system_state.get_mut(world);

                    if let Some((mut record, _)) =
                        query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
                    {
                        let record = record.as_mut();
                        system.run(Some(record), params);
                    }
                }
                system_state.apply(world);
            })
            .await;
    } else {
        async_world
            .apply(move |world: &mut World| {
                let mut system_state: SystemState<(S::Param)> = SystemState::new(world);
                {
                    let (params) = system_state.get_mut(world);
                    system.run(None, params);
                }
                system_state.apply(world);
            })
            .await;
    }
}

#[cfg(feature = "surrealdb")]
pub async fn get_records<T: FluxRecord>(
    db: Arc<Mutex<Surreal<Any>>>,
) -> anyhow::Result<Vec<(Id, T)>> {
    use surrealdb::types::SurrealValue;

    let o: Vec<SerdeWrapper<TypedRecord<T>>> = db.lock().await.select(T::short_type_path()).await.unwrap();
    let o = o
        .iter()
        .map(|record| (Id::from(&record.0.id.key.clone().into_value().as_string().unwrap()), record.0.record.clone()))
        .collect();
    Ok(o)
}

pub trait DbCommandsExt {
    //fn add_or_get<'a, T, S, SMarker>(&mut self, id: &Id, system: S) where T: Component<Mutability = Mutable> + Typed + DeserializeOwned, S: SystemParamFunction<SMarker, In = InMut<'a, T>, Out = ()>, SMarker: 'static, <S as bevy::prelude::SystemParamFunction<SMarker>>::Param: 'static;

    fn upsert_record<'a, T, S, SM>(&mut self, id: Id, record: T, system: S)
    where
        S: UpsertSys<T, SM>;

    fn try_get_record<T, O, S, SM>(&mut self, id: Id, system: S)
    where
        S: TryGetRecordSys<T, O, SM>;

    fn run<Task, Output, Spawnable>(&mut self, task: Spawnable)
    where
        Task: Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
        Spawnable: FnOnce(AsyncWorld) -> Task + Send + 'static;
}

// implement our trait for Bevy's `Commands`
impl<'w, 's> DbCommandsExt for Commands<'w, 's> {
    /*
    fn add_or_get<'a, T, S, SMarker>(&mut self, id: &Id, mut system: S) where T: Component<Mutability = Mutable> + Reflect + Typed + DeserializeOwned, S: SystemParamFunction<SMarker, In = InMut<'a, T>, Out = ()>, SMarker: 'static, <S as bevy::prelude::SystemParamFunction<SMarker>>::Param: 'static {
        let id = id.clone();

        self.queue(move |world: &mut World| {

            let mut system_state: SystemState<(Res<AsyncRunner>, Tasks, Res<DBConfig>, Query<(Mut<T>, &DBRecord)>, S::Param)> = SystemState::new(world);
            let (runner, tasks, db, mut query, params) = system_state.get_mut(world);
            if let Some((mut record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
            {
                system.run(&mut record, params);
            } else {
                let db = db.db.clone();
                let async_world = runner.get_async_world();

                tasks.spawn_auto(async move |x| {
                    let record: Option<T> = db.select((T::short_type_path(), id.id.clone())).await.unwrap();
                    if let Some(mut record) = record {
                        async_world.apply(move |world: &mut World| {
                            spawn_record_with_callback(world, &id, record, system);
                        });
                    }
                });
            }
        });
    }
    */

    fn run<Task, Output, Spawnable>(&mut self, task: Spawnable)
    where
        Task: Future<Output = Output> + Send + 'static,
        Output: Send + 'static,
        Spawnable: FnOnce(AsyncWorld) -> Task + Send + 'static,
    {
        self.queue(move |world: &mut World| {
            let mut system_state: SystemState<(Res<AsyncRunner>, Tasks)> = SystemState::new(world);
            {
                let (runner, tasks) = system_state.get_mut(world);

                let async_world = runner.get_async_world();
                tasks.spawn_auto(async move |_| {
                    task(async_world).await;
                });
            }
            system_state.apply(world);
            /*
            let mut system_state: SystemState<(Res<AsyncRunner>)> = SystemState::new(world);
            let (runner) = system_state.get_mut(world);
            runner.run(task);
            */
        });
    }

    fn upsert_record<'a, T, S, SM>(&mut self, id: Id, record: T, mut system: S)
    where
        S: UpsertSys<T, SM>,
    {
        self.queue(UpsertRecordWithCallback::new(id, record, system));
    }

    fn try_get_record<T, O, S, SM>(&mut self, id: Id, mut system: S)
    where
        S: TryGetRecordSys<T, O, SM>,
    {
        let id = id.clone();

        self.queue(move |world: &mut World| {
            let mut system_state: SystemState<(
                Res<AsyncRunner>,
                Tasks,
                Res<DBConfig>,
                Query<(Mut<T>, &DBRecord)>,
                S::Param,
            )> = SystemState::new(world);
            {
                let (runner, tasks, db, mut query, params) = system_state.get_mut(world);
                if let Some((mut record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
                {
                    system.run(Some(&mut record), params);
                } else {
                    #[cfg(feature = "surrealdb")]
                    {
                        let db = db.db.clone();
                        let async_world = runner.get_async_world();
                        tasks.spawn_auto(async move |_| {
                            try_get_record(async_world, db, id, system).await;
                        });
                    }
                }
            }
            system_state.apply(world);
        });
    }
}

fn spawn_record_with_callback<T, S, SM>(world: &mut World, id: Id, record: T, mut system: S)
where
    S: UpsertSys<T, SM>,
{
    spawn_record(world, id, record);

    let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> =
        SystemState::new(world);
    {
        let (mut query, params) = system_state.get_mut(world);

        if let Some((mut record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == id) {
            system.run(&mut record, params);
        }
    }

    system_state.apply(world);
}

fn spawn_record<T>(world: &mut World, id: Id, record: T)
where
    T: Component<Mutability = Mutable> + Reflect + Typed + DeserializeOwned,
{
    info!("Spawning record of type {} with ID: {:#}", T::short_type_path(), id);

    world.spawn((DBRecord { id }, record));
}

/*
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
*/
