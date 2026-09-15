//! Shared graph snapshots and one bounded scheduler.
use super::*;
use bevy::ecs::{component::Tick, system::SystemState};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub(super) type Snapshot = Option<Arc<dyn PartialReflect>>;
type Key = (Entity, String);
type PathKey = BindingPath;
type ReadState = SystemState<(
    Query<'static, 'static, (Entity, All<&'static dyn Reactive>)>,
    Res<'static, DBConfig>,
)>;

pub(super) fn snapshot_copy(value: &Snapshot) -> BindingValue {
    value.as_ref().map(|value| value.as_ref().clone_value())
}
pub(super) fn snapshot_equal(a: &Snapshot, b: &Snapshot) -> Option<bool> {
    match (a, b) {
        (None, None) => Some(true),
        (Some(a), Some(b)) if Arc::ptr_eq(a, b) => Some(true),
        (Some(a), Some(b)) => a.as_ref().reflect_partial_eq(b.as_ref()),
        _ => Some(false),
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Dependency {
    pub key: Key,
    ticks: Option<(Tick, Tick)>,
    generation: u64,
}

pub(super) struct CachedPath {
    pub value: Snapshot,
    pub version: u64,
    pub route_version: u64,
    pub dependencies: Vec<Dependency>,
    mappings: Vec<(Id, Option<Entity>)>,
    dirty: bool,
    used: u64,
}

struct TrackingResolver<'a, 'w, 's> {
    query: &'a Query<'w, 's, (Entity, All<&'static dyn Reactive>)>,
    db: &'a DBConfig,
    dependencies: RefCell<Vec<Dependency>>,
    mappings: RefCell<Vec<(Id, Option<Entity>)>>,
    generations: &'a HashMap<Key, u64>,
}

impl TrackingResolver<'_, '_, '_> {
    fn ticks(&self, key: &Key) -> Option<(Tick, Tick)> {
        self.query
            .get(key.0)
            .ok()?
            .1
            .iter()
            .find(|value| value.reflect_short_type_path() == key.1)
            .map(|value| (value.added(), value.last_changed()))
    }
}

impl EntityResolver for TrackingResolver<'_, '_, '_> {
    fn get_reactive(
        &self,
        entity: Entity,
        component: &str,
    ) -> Option<(Option<String>, Box<dyn PartialReflect>)> {
        let key = (entity, component.to_owned());
        self.dependencies.borrow_mut().push(Dependency {
            ticks: self.ticks(&key),
            generation: self.generations.get(&key).copied().unwrap_or_default(),
            key,
        });
        self.query
            .get(entity)
            .ok()?
            .1
            .iter()
            .find(|value| value.reflect_short_type_path() == component)
            .map(|value| (None, value.clone_value()))
    }
    fn resolve_id(&self, id: &Id) -> Option<Entity> {
        let entity = self.db.get_entity(id);
        self.mappings.borrow_mut().push((id.clone(), entity));
        entity
    }
}

pub(super) struct PathReads {
    state: ReadState,
    paths: HashMap<PathKey, CachedPath>,
    generations: HashMap<Key, u64>,
    frame: u64,
    revision: u64,
    #[cfg(test)]
    pub walks: usize,
}

impl PathReads {
    fn new(world: &mut World) -> Self {
        Self {
            state: SystemState::new(world),
            paths: HashMap::new(),
            generations: HashMap::new(),
            frame: 0,
            revision: 0,
            #[cfg(test)]
            walks: 0,
        }
    }

    pub fn read(&mut self, path: &BindingPath, world: &World) -> Result<&CachedPath> {
        ensure!(
            world.contains_resource::<DBConfig>(),
            "Binding path requires DBConfig"
        );
        let (query, db) = self.state.get(world);
        let resolver = TrackingResolver {
            query: &query,
            db: &db,
            dependencies: Default::default(),
            mappings: Default::default(),
            generations: &self.generations,
        };
        let valid = self.paths.get(path).is_some_and(|cached| {
            !cached.dirty
                && cached.dependencies.iter().all(|dep| {
                    resolver.ticks(&dep.key) == dep.ticks
                        && self.generations.get(&dep.key).copied().unwrap_or_default()
                            == dep.generation
                })
                && cached
                    .mappings
                    .iter()
                    .all(|(id, entity)| db.get_entity(id) == *entity)
        });
        if !valid {
            let value: Snapshot = match resolver.get_reactive(path.entity, &path.component) {
                Some((_, root)) => walk(root, &path.parsed, &resolver)?.map(Arc::from),
                None => None,
            };
            let dependencies = resolver.dependencies.into_inner();
            let mappings = resolver.mappings.into_inner();
            let old = self.paths.get(path);
            let route_changed = old.is_none_or(|old| {
                old.mappings != mappings
                    || old.value.is_some() != value.is_some()
                    || old.dependencies.len() != dependencies.len()
                    || old.dependencies.iter().zip(&dependencies).any(|(a, b)| {
                        a.key != b.key || a.ticks.map(|t| t.0) != b.ticks.map(|t| t.0)
                    })
            });
            // Versions must not be reused after eviction (e.g. an error/recovery).
            self.revision = self.revision.wrapping_add(1);
            let version = old
                .filter(|old| !route_changed && snapshot_equal(&old.value, &value) == Some(true))
                .map_or(self.revision, |old| old.version);
            let route_version = old
                .filter(|_| !route_changed)
                .map_or(self.revision, |old| old.route_version);
            self.paths.insert(
                path.clone(),
                CachedPath {
                    value,
                    version,
                    route_version,
                    dependencies,
                    mappings,
                    dirty: false,
                    used: self.frame,
                },
            );
            #[cfg(test)]
            {
                self.walks += 1;
            }
        }
        let cached = self.paths.get_mut(path).unwrap();
        cached.used = self.frame;
        Ok(cached)
    }

    pub fn invalidate(&mut self, dependencies: &[Dependency]) {
        for dependency in dependencies {
            let generation = self.generations.entry(dependency.key.clone()).or_default();
            *generation = generation.wrapping_add(1);
        }
    }
    pub fn invalidate_all(&mut self) {
        for cached in self.paths.values_mut() {
            cached.dirty = true;
        }
    }
    pub fn dependency_keys(&self) -> HashSet<Key> {
        self.paths
            .values()
            .flat_map(|path| path.dependencies.iter().map(|dep| dep.key.clone()))
            .collect()
    }
    fn dependencies(&self, path: &BindingPath) -> Vec<Key> {
        self.paths
            .get(path)
            .map(|cached| {
                cached
                    .dependencies
                    .iter()
                    .map(|dep| dep.key.clone())
                    .collect()
            })
            .unwrap_or_else(|| vec![(path.entity, path.component.clone())])
    }
}

impl BindingGraph {
    pub(super) fn change_driven(&self) -> bool {
        self.links.is_empty()
            && self.nodes.iter().all(|node| node.system.is_none())
            && self
                .sinks
                .iter()
                .all(|sink| sink.policy == BindingWritePolicy::OnSourceChange)
    }
}

pub fn evaluate_binding_graphs(world: &mut World) {
    world.init_resource::<BindingGraphs>();
    if world.resource::<BindingGraphs>().runtime.is_none() {
        let reads = PathReads::new(world);
        world.resource_mut::<BindingGraphs>().runtime = Some(reads);
    }
    world.resource_scope(|world, mut graphs: Mut<BindingGraphs>| {
        graphs
            .graphs
            .retain(|graph| world.get_entity(graph.owner).is_ok());
        let mut reads = graphs.runtime.take().unwrap();
        reads.frame = reads.frame.wrapping_add(1);
        let count = graphs.graphs.len();
        let mut dependents: HashMap<Key, HashSet<usize>> = HashMap::new();
        // Settle direct deliveries before ECS computations consume their inputs.
        let mut queue: VecDeque<usize> = (0..count)
            .filter(|&index| graphs.graphs[index].graph.change_driven())
            .chain((0..count).filter(|&index| !graphs.graphs[index].graph.change_driven()))
            .collect();
        let mut queued = vec![true; count];
        let mut ran = vec![false; count];
        // Give each graph its own share: one cycle must not starve unrelated work.
        let limit = (1024 / count.max(1)).max(64);
        let mut visits = vec![0_usize; count];
        for (index, owned) in graphs.graphs.iter().enumerate() {
            for node in &owned.graph.nodes {
                if let Some(path) = &node.source_path {
                    for key in reads.dependencies(path) {
                        dependents.entry(key).or_default().insert(index);
                    }
                }
            }
        }
        while let Some(index) = queue.pop_front() {
            queued[index] = false;
            if visits[index] == limit {
                let message = "Binding cascade exceeded its work budget (possible cycle)";
                if graphs.graphs[index].error.as_deref() != Some(message) {
                    tracing::warn!(owner = ?graphs.graphs[index].owner, "{message}");
                }
                graphs.graphs[index].error = Some(message.into());
                continue;
            }
            visits[index] += 1;
            let owned = &mut graphs.graphs[index];
            if ran[index] && !owned.graph.change_driven() {
                continue;
            }
            ran[index] = true;
            let mut changed = HashSet::new();
            let error = owned
                .graph
                .evaluate(world, &mut reads, &mut changed)
                .err()
                .map(|e| e.to_string());
            if error != owned.error {
                if let Some(error) = &error {
                    tracing::warn!(owner = ?owned.owner, %error, "Binding graph failed");
                }
            }
            owned.error = error;
            for node in &owned.graph.nodes {
                if let Some(path) = &node.source_path {
                    for key in reads.dependencies(path) {
                        dependents.entry(key).or_default().insert(index);
                    }
                }
            }
            for key in changed {
                if let Some(indices) = dependents.get(&key) {
                    for &dependent in indices {
                        if !queued[dependent] && graphs.graphs[dependent].graph.change_driven() {
                            queued[dependent] = true;
                            queue.push_front(dependent);
                        }
                    }
                }
            }
        }
        reads.paths.retain(|_, cached| cached.used == reads.frame);
        let live = reads.dependency_keys();
        reads.generations.retain(|key, _| live.contains(key));
        graphs.runtime = Some(reads);
    });
}
