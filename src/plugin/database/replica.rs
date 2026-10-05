use crate::prelude::*;
use crate::schema::{Catalog, GraphRef, ReplicaCache, ReplicaEnvelope};
use bevy::prelude::*;
use serde::de::DeserializeOwned;
use std::{collections::BTreeMap, marker::PhantomData};

pub struct ReplicaProjection<T> {
    cache: ReplicaCache,
    entities: BTreeMap<uuid::Uuid, Entity>,
    marker: PhantomData<T>,
}

impl<T: Component + DeserializeOwned + crate::schema::Schema> ReplicaProjection<T> {
    pub fn new(catalog: &Catalog, graph: GraphRef, epoch: uuid::Uuid, capacity: usize) -> anyhow::Result<Self> {
    anyhow::ensure!(graph == catalog.graph_for::<T>()?, "Replica graph does not match the Rust type");
        Ok(Self { cache: ReplicaCache::new(catalog, graph, epoch, capacity)?, entities: BTreeMap::new(), marker: PhantomData })
    }

    pub fn revision(&self, record_id: uuid::Uuid) -> Option<u64> {
        self.cache.revision(record_id)
    }

    pub fn apply_authoritative(&mut self, world: &mut World, catalog: &Catalog, envelope: ReplicaEnvelope) -> anyhow::Result<()> {
        let record_id = envelope.record_id;
        anyhow::ensure!(!record_id.is_nil(), "Replica record ID must not be nil");
        let mut next = self.cache.clone();
        next.apply_authoritative(catalog, envelope)?;
        let value = next.value(record_id).map(|value| serde_json::from_value::<T>(value.clone())).transpose()?;
        let identity = Id::from(&record_id.to_string());
        let entities = world.query::<(Entity, &DBRecord)>().iter(world)
            .filter(|(_, record)| record.id == identity).map(|(entity, _)| entity).collect::<Vec<_>>();
        anyhow::ensure!(entities.len() <= 1, "Multiple ECS entities have the same record identity");
        let entity = entities.first().copied();
        match (entity, value) {
            (Some(entity), Some(value)) => {
                world.entity_mut(entity).insert(value);
                self.entities.insert(record_id, entity);
            }
            (None, Some(value)) => {
                let entity = world.spawn((DBRecord { id: identity }, value)).id();
                self.entities.insert(record_id, entity);
            }
            (Some(entity), None) => {
                world.entity_mut(entity).remove::<T>();
                self.entities.remove(&record_id);
            }
            (None, None) => { self.entities.remove(&record_id); }
        }
        self.cache = next;
        Ok(())
    }

    pub fn reset(&mut self, world: &mut World, catalog: &Catalog, graph: GraphRef, epoch: uuid::Uuid) -> anyhow::Result<()> {
        anyhow::ensure!(graph == catalog.graph_for::<T>()?, "Replica graph does not match the Rust type");
        let mut next = self.cache.clone();
        next.reset(catalog, graph, epoch)?;
        for (record_id, entity) in &self.entities {
            if world.get::<DBRecord>(*entity).is_some_and(|record| record.id == Id::from(&record_id.to_string())) {
                world.entity_mut(*entity).remove::<T>();
            }
        }
        self.entities.clear();
        self.cache = next;
        Ok(())
    }
}