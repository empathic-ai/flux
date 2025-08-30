use std::marker::PhantomData;
#[cfg(feature = "surrealdb")]
use std::sync::Arc;

use bevy::{ecs::{component::Mutable, system::{ExclusiveSystemParamFunction, ParamBuilder, RunSystemOnce, SystemParam, SystemState}}, prelude::*, reflect::Typed};
use bevy_async_ecs::AsyncWorld;
#[cfg(feature = "surrealdb")]
use futures::lock::Mutex;
use serde::{de::DeserializeOwned, Serialize};
use crate::prelude::*;
use bevy_wasm_tasks::Tasks;

#[cfg(feature = "surrealdb")]
use surrealdb::{engine::any::Any, Surreal};

trait RecordComp = Component<Mutability = Mutable> + Typed + DeserializeOwned + Serialize + Clone;
trait UpsertSys<T, SM> = SystemParamFunction<SM> + 'static where
T: RecordComp,
SM: Send + Sync + 'static,
// enforce that S’s output is `()`
<Self as SystemParamFunction<SM>>::Out: std::convert::Into<()>,
// for *every* lifetime 'a, S::In must be InMut<'a, T>
for<'a> <Self as SystemParamFunction<SM>>::In: SystemInput<Inner<'a> = &'a mut T>,
// S’s Param must be a SystemParam and be 'static
<Self as SystemParamFunction<SM>>::Param: SystemParam + 'static;

trait GetSys<T, O, SM> = SystemParamFunction<SM, Out = O> + 'static where
T: RecordComp,
SM: Send + Sync + 'static,
// enforce that S’s output is `()`
//<Self as SystemParamFunction<SM>>::Out: std::convert::Into<()>,
// for *every* lifetime 'a, S::In must be InMut<'a, T>
for<'a> <Self as SystemParamFunction<SM>>::In: SystemInput<Inner<'a> = Option<&'a mut T>>,
// S’s Param must be a SystemParam and be 'static
<Self as SystemParamFunction<SM>>::Param: SystemParam + 'static;

pub struct UpsertRecord<T> where T: RecordComp
{
	id: Id,
	record: T
}

impl<T> UpsertRecord<T> where T: RecordComp
{
	fn new(id: Id, record: T) -> Self {
		Self {
			id,
			record
		}
	}
}

impl<T> Command for UpsertRecord<T> where T: Component<Mutability = Mutable> + Typed + DeserializeOwned + Serialize + Clone
{
	fn apply(mut self, world: &mut World) {

		let type_name = T::short_type_path();

		let mut is_record = false;
		
		let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, Tasks, Res<DBConfig>)> = SystemState::new(world);
		{
			{
				let (mut query, tasks, db_config) = system_state.get_mut(world);
				let db = db_config.db.clone();
		
				let record = self.record.clone();
				tasks.spawn_auto(async move |_| {

					//info!("Adding record of type {} to database!", type_name);
					let c: Option<Record> = db.lock().await
						.upsert((type_name.to_string(), self.id.to_string()))
						.content(record)
						.await
						.unwrap();
				});

				if let Some((mut _record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == self.id)
				{
					if _record.reflect_partial_eq(self.record.as_partial_reflect()).is_none_or(|x| !x) {
						_record.apply(self.record.as_partial_reflect());
					}
					is_record = true;
				}
			}
			system_state.apply(world);
		}
	
		if !is_record {
			spawn_record(world, &self.id, self.record);
		}
	}
}

pub struct UpsertRecordWithCallback<T, S, SM> where S: UpsertSys<T, SM>
{
	id: Id,
	record: T,
	system: S,
	smarker_data: PhantomData<SM>
}

impl<T, S, SM> UpsertRecordWithCallback<T, S, SM> where S: UpsertSys<T, SM>
{
	fn new(id: Id, record: T, system: S) -> Self {
		Self {
			id,
			record,
			system,
			smarker_data: PhantomData
		}
	}
}

impl<T, S, SM> Command for UpsertRecordWithCallback<T, S, SM> where S: UpsertSys<T, SM>
{
	fn apply(mut self, world: &mut World) {
		let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> = SystemState::new(world);
		{
			{
				let (mut query, params) = system_state.get_mut(world);
				if let Some((mut _record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == self.id)
				{
					_record.apply(self.record.as_partial_reflect());
					self.system.run(&mut _record, params);
					return;
				}
			}
			system_state.apply(world);
		}
	
		spawn_record_with_callback(world, &self.id, self.record, self.system);
	}
}

pub trait AsyncDbCommandsExt {
	async fn upsert_record<T>(&self, id: Id, record: T) -> Id where T: RecordComp;

	async fn upsert_record_with_callback<T, S, SM>(&self, id: Id, record: T, system: S) where S: UpsertSys<T, SM>;

	/// Applies the given `Command` to the world.
	async fn get_record<T, O, S, SM>(&self, id: Id, system: S) where S: GetSys<T, O, SM>;
}

impl AsyncDbCommandsExt for AsyncWorld {

	async fn upsert_record<T>(&self, id: Id, record: T) -> Id where T: RecordComp {
		self.apply(UpsertRecord::new(id, record)).await;
		id
	}

	async fn upsert_record_with_callback<T, S, SM>(&self, id: Id, record: T, mut system: S) where S: UpsertSys<T, SM> {
		self.apply(UpsertRecordWithCallback::new(id, record, system)).await;
	}

	/// Applies the given `Command` to the world.
	async fn get_record<T, O, S, SM>(&self, id: Id, mut system: S) where S: GetSys<T, O, SM> {

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
			}).await;
	
			output_rx.recv().await;
		}
	}

}

#[cfg(feature = "surrealdb")]
async fn get_record<T, O, S, SM>(async_world: AsyncWorld, db: Arc<Mutex<Surreal<Any>>>, id: Id, mut system: S) where S: GetSys<T, O, SM> {
	let record: Option<T> = db.lock().await.select((T::short_type_path(), id.to_pretty_string())).await.unwrap();
	if let Some(mut record) = record {
		async_world.apply(move |world: &mut World| {
			spawn_record(world, &id, record);

			let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> = SystemState::new(world);
			{
				let (mut query, params) = system_state.get_mut(world);
		
				if let Some((mut record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == id)
				{
					let record = record.as_mut();
					system.run(Some(record), params);
				}
			}
			system_state.apply(world);
		}).await;
	} else {
		async_world.apply(move |world: &mut World| {
			let mut system_state: SystemState<(S::Param)> = SystemState::new(world);
			{
				let (params) = system_state.get_mut(world);
				system.run(None, params);
			}
			system_state.apply(world);
		}).await;
	}
}

pub trait DbCommandsExt {
	//fn add_or_get<'a, T, S, SMarker>(&mut self, id: &Id, system: S) where T: Component<Mutability = Mutable> + Typed + DeserializeOwned, S: SystemParamFunction<SMarker, In = InMut<'a, T>, Out = ()>, SMarker: 'static, <S as bevy::prelude::SystemParamFunction<SMarker>>::Param: 'static;

	fn upsert_record<'a, T, S, SM>(&mut self, id: Id, record: T, system: S) where S: UpsertSys<T, SM>;

	fn get_record<T, O, S, SM>(&mut self, id: Id, system: S) where S: GetSys<T, O, SM>;

	fn run<Task, Output, Spawnable>(&mut self, task: Spawnable)     where
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

	fn upsert_record<'a, T, S, SM>(&mut self, id: Id, record: T, mut system: S) where S: UpsertSys<T, SM> {
		self.queue(UpsertRecordWithCallback::new(id, record, system));
	}

	fn get_record<T, O, S, SM>(&mut self, id: Id, mut system: S) where S: GetSys<T, O, SM> {
		let id = id.clone();

		self.queue(move |world: &mut World| {
			
			let mut system_state: SystemState<(Res<AsyncRunner>, Tasks, Res<DBConfig>, Query<(Mut<T>, &DBRecord)>, S::Param)> = SystemState::new(world);
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
							get_record(async_world, db, id, system).await;
						});
					}
				}
			}
			system_state.apply(world);
		});
	}
}

fn spawn_record_with_callback<T, S, SM>(world: &mut World, id: &Id, record: T, mut system: S) where S: UpsertSys<T, SM> {
	spawn_record(world, id, record);

	let mut system_state: SystemState<(Query<(Mut<T>, &DBRecord)>, S::Param)> = SystemState::new(world);
	{
		let (mut query, params) = system_state.get_mut(world);

		if let Some((mut record, _)) = query.iter_mut().find(|(_, db_rec)| db_rec.id == *id)
		{
			system.run(&mut record, params);
		}
	}

	system_state.apply(world);
}

fn spawn_record<T>(world: &mut World, id: &Id, record: T) where T: Component<Mutability = Mutable> + Reflect + Typed + DeserializeOwned {
	world.spawn((DBRecord { id: id.clone() }, record));
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