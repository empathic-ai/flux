//! Composable, snapshot-based ECS dataflow. See `docs/binding-graphs.md`.
mod process;
mod expression;
pub use expression::{BindingExpr, BindingExprInputs, IntoBindingExpr, process, process_system};
pub use process::{ProcessFn, ProcessInputs};
use std::collections::HashSet;

use anyhow::{Result, anyhow, ensure};
use bevy::{
    ecs::system::{BoxedSystem, ReadOnlySystem},
    prelude::*,
    reflect::{DynamicList, PartialReflect, ReflectRef},
};
use bevy_trait_query::All;

use crate::prelude::{
    DBConfig, EntityResolver, Id, OptionalParsedPath, PathStep, PathWalker, Reactive,
    ReactivesQuery, apply_value_at_target_path,
};

/// Common result type for graph construction and macro expansions.
pub type BindingResult<T> = anyhow::Result<T>;

/// Missing is distinct from a reflected `Option::None`. Operators may recover it.
pub type BindingValue = Option<Box<dyn PartialReflect>>;
pub type BindingInputs = Vec<BindingValue>;
type Reader = Box<dyn ReadOnlySystem<In = In<BindingInputs>, Out = Result<BindingValue>>>;
type Writer = BoxedSystem<In<BindingValue>, Result<()>>;

fn copy_value(value: &BindingValue) -> BindingValue {
    value.as_ref().map(|value| value.clone_value())
}

fn equal(a: &BindingValue, b: &BindingValue) -> Option<bool> {
    match (a, b) {
        (None, None) => Some(true),
        (Some(a), Some(b)) => a.reflect_partial_eq(b.as_ref()),
        _ => Some(false),
    }
}

/// A validated reflected location. Names follow the existing Flux path grammar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingPath {
    entity: Entity,
    component: String,
    path: Option<String>,
    parsed: OptionalParsedPath,
}

impl BindingPath {
    pub fn new(entity: Entity, component: impl Into<String>, path: Option<&str>) -> Result<Self> {
        Ok(Self {
            entity,
            component: component.into(),
            path: path.map(str::to_owned),
            parsed: OptionalParsedPath::parse(path.unwrap_or(""))
                .map_err(|error| anyhow!("Invalid binding path: {error}"))?,
        })
    }

    // Returns the value at the end of the path, or None if the path can't be fully resolved
    fn reader(&self) -> Reader {
        let location = self.clone();
        Box::new(IntoSystem::into_system(
            move |_: In<BindingInputs>,
                  query: Query<(Entity, All<&'static dyn Reactive>)>,
                  db: Res<DBConfig>| {
                let resolver = ReadResolver {
                    query: &query,
                    db: &db,
                };
                let Some((_, root)) = resolver.get_reactive(location.entity, &location.component)
                else {
                    return Ok(None);
                };
                walk(root, &location.parsed, &resolver)
            },
        ))
    }

    // Returns a collection of entity identifiers along the path
    // Used to detect if the binding path has changed
    fn route_reader(&self) -> Reader {
        let location = self.clone();
        Box::new(IntoSystem::into_system(
            move |_: In<BindingInputs>,
                  query: Query<(Entity, All<&'static dyn Reactive>)>,
                  db: Res<DBConfig>| {
                let resolver = ReadResolver {
                    query: &query,
                    db: &db,
                };
                let Some((_, root)) = resolver.get_reactive(location.entity, &location.component)
                else {
                    return Ok(None);
                };
                let mut route = vec![location.entity.to_bits()];
                let mut walker = PathWalker::new(root, &location.parsed, &resolver);
                for step in &mut walker {
                    if let PathStep::EntityJump { entity, .. } = step {
                        route.push(entity.to_bits());
                    }
                }
                Ok(walker
                    .stop_reason()
                    .is_none()
                    .then(|| Box::new(route) as Box<dyn PartialReflect>))
            },
        ))
    }

    fn writer(&self) -> Writer {
        let location = self.clone();
        Box::new(IntoSystem::into_system(
            move |In(value): In<BindingValue>, mut query: ReactivesQuery, db: Res<DBConfig>| {
                let Some(value) = value else { return Ok(()) };
                apply_value_at_target_path(
                    &mut query,
                    &db,
                    location.entity,
                    location.component.clone(),
                    location.path.clone(),
                    value,
                    &mut HashSet::new(),
                )
            },
        ))
    }
}

struct ReadResolver<'a, 'w, 's> {
    query: &'a Query<'w, 's, (Entity, All<&'static dyn Reactive>)>,
    db: &'a DBConfig,
}

impl EntityResolver for ReadResolver<'_, '_, '_> {
    fn get_reactive(
        &self,
        entity: Entity,
        component: &str,
    ) -> Option<(Option<String>, Box<dyn PartialReflect>)> {
        self.query
            .get(entity)
            .ok()?
            .1
            .iter()
            .find(|value| value.reflect_short_type_path() == component)
            .map(|value| (None, value.clone_value()))
    }
    fn resolve_id(&self, id: &Id) -> Option<Entity> {
        self.db.get_entity(id)
    }
}

fn walk(
    root: Box<dyn PartialReflect>,
    path: &OptionalParsedPath,
    resolver: &impl EntityResolver,
) -> Result<BindingValue> {
    let mut walker = PathWalker::new(root, path, resolver);
    for _ in &mut walker {}
    Ok(walker
        .stop_reason()
        .is_none()
        .then(|| walker.current_value().clone_value()))
}

/// Handles belong to exactly one graph. Only earlier nodes may be connected,
/// making cycles impossible through the builder API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingNode {
    graph: uuid::Uuid,
    index: usize,
}

struct Node {
    source_path: Option<BindingPath>,
    name: String,
    inputs: Vec<usize>,
    system: Reader,
}
struct Sink {
    target_path: BindingPath,
    input: usize,
    read: Reader,
    system: Writer,
}

/// A runtime graph, owned by an entity after installation. Drop/despawn cleans
/// up its systems; no registered-system entities or global subscriptions leak.
pub struct BindingGraph {
    id: uuid::Uuid,
    nodes: Vec<Node>,
    sinks: Vec<Sink>,
    links: Vec<TwoWayLink>,
}

impl Default for BindingGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingGraph {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            nodes: Vec::new(),
            sinks: Vec::new(),
            links: Vec::new(),
        }
    }

    fn check_node(&self, node: BindingNode) -> Result<usize> {
        ensure!(
            node.graph == self.id && node.index < self.nodes.len(),
            "Binding node belongs to another graph"
        );
        Ok(node.index)
    }

    fn push(
        &mut self,
        name: String,
        inputs: &[BindingNode],
        system: Reader,
    ) -> Result<BindingNode> {
        ensure!(
            !system.has_deferred(),
            "Binding computations cannot contain deferred writes (Commands)"
        );
        let inputs = inputs
            .iter()
            .map(|node| self.check_node(*node))
            .collect::<Result<_>>()?;
        let node = BindingNode {
            graph: self.id,
            index: self.nodes.len(),
        };
        self.nodes.push(Node {
            source_path: None,
            name,
            inputs,
            system,
        });
        Ok(node)
    }

    /// Extension point for sources, filters, joins, projections and reductions.
    /// Query/Res dependencies are refreshed every evaluation, including removals.
    /// Use complete query results, not Changed<T>, to produce a state snapshot.
    pub fn system<S, M>(
        &mut self,
        name: impl Into<String>,
        inputs: &[BindingNode],
        system: S,
    ) -> Result<BindingNode>
    where
        S: IntoSystem<In<BindingInputs>, Result<BindingValue>, M>,
        S::System: ReadOnlySystem,
    {
        self.push(
            name.into(),
            inputs,
            Box::new(IntoSystem::into_system(system)),
        )
    }

    pub fn source(&mut self, path: BindingPath) -> Result<BindingNode> {
        let node = self.push(format!("{path:?}"), &[], path.reader())?;
        self.nodes[node.index].source_path = Some(path);
        Ok(node)
    }

    pub fn constant<T: PartialReflect>(&mut self, value: T) -> Result<BindingNode> {
        self.map("constant", &[], move |_| Ok(Some(value.clone_value())))
    }

    /// Pure value operation. The closure can handle missing inputs explicitly.
    pub fn map(
        &mut self,
        name: impl Into<String>,
        inputs: &[BindingNode],
        map: impl Fn(BindingInputs) -> Result<BindingValue> + Send + Sync + 'static,
    ) -> Result<BindingNode> {
        self.system(name, inputs, move |In(inputs): In<BindingInputs>| {
            map(inputs)
        })
    }

    /// Typed single-input adapter; conversion failures are graph diagnostics.
    pub fn map_value<T: FromReflect, U: PartialReflect>(
        &mut self,
        input: BindingNode,
        map: impl Fn(T) -> Result<U> + Send + Sync + 'static,
    ) -> Result<BindingNode> {
        self.map("map_value", &[input], move |mut values| {
            let Some(value) = values.remove(0) else {
                return Ok(None);
            };
            let value = T::from_reflect(value.as_ref())
                .ok_or_else(|| anyhow!("map_value expected {}", std::any::type_name::<T>()))?;
            Ok(Some(Box::new(map(value)?)))
        })
    }

    /// Filters a list snapshot, preserving order. For ECS-aware predicates use
    /// `system` with a Query/Res and return the filtered list snapshot.
    pub fn filter<T: FromReflect>(
        &mut self,
        input: BindingNode,
        predicate: impl Fn(&T) -> bool + Send + Sync + 'static,
    ) -> Result<BindingNode> {
        self.map("filter", &[input], move |mut values| {
            let Some(value) = values.remove(0) else {
                return Ok(None);
            };
            let ReflectRef::List(list) = value.reflect_ref() else {
                return Err(anyhow!("filter expects a list"));
            };
            let mut output = DynamicList::default();
            for item in list.iter() {
                let typed = T::from_reflect(item)
                    .ok_or_else(|| anyhow!("filter expected {}", std::any::type_name::<T>()))?;
                if predicate(&typed) {
                    output.push_box(item.clone_value());
                }
            }
            Ok(Some(Box::new(output)))
        })
    }

    /// Continue a reflected path after any operator, including Id entity jumps.
    pub fn path(&mut self, input: BindingNode, path: &str) -> Result<BindingNode> {
        let parsed = OptionalParsedPath::parse(path)
            .map_err(|error| anyhow!("Invalid binding path: {error}"))?;
        self.system(
            format!("path {path}"),
            &[input],
            move |In(mut values): In<BindingInputs>,
                  query: Query<(Entity, All<&'static dyn Reactive>)>,
                  db: Res<DBConfig>| {
                let Some(root) = values.remove(0) else {
                    return Ok(None);
                };
                walk(
                    root,
                    &parsed,
                    &ReadResolver {
                        query: &query,
                        db: &db,
                    },
                )
            },
        )
    }


    /// Applies after every computation completes. Missing values retain the target.
    /// The reflected writer suppresses equal writes, and retries missing targets.
    pub fn bind(&mut self, input: BindingNode, target: BindingPath) -> Result<&mut Self> {
        let input = self.check_node(input)?;
        self.sinks.push(Sink {
            target_path: target.clone(),
            input,
            read: target.reader(),
            system: target.writer(),
        });
        Ok(self)
    }

    /// Direct two-way link. Noninvertible computations require an explicit edit
    /// command; they must never be reversed by guessing a source list/index.
    pub fn two_way(
        &mut self,
        source: BindingPath,
        target: BindingPath,
        conflict: BindingConflict,
    ) -> &mut Self {
        self.links.push(TwoWayLink::new(source, target, conflict));
        self
    }

    /// Two-way conversion with explicit, fallible forward and backward mappings.
    /// Callers must ensure round trips preserve values (or document normalization).
    pub fn two_way_with<T: FromReflect + PartialReflect, U: FromReflect + PartialReflect>(
        &mut self,
        source: BindingPath,
        target: BindingPath,
        conflict: BindingConflict,
        forward: impl Fn(T) -> Result<U> + Send + Sync + 'static,
        backward: impl Fn(U) -> Result<T> + Send + Sync + 'static,
    ) -> &mut Self {
        let mut link = TwoWayLink::new(source, target, conflict);
        link.forward = Box::new(move |value| {
            let value =
                T::from_reflect(value).ok_or_else(|| anyhow!("Invalid two-way source type"))?;
            Ok(Box::new(forward(value)?))
        });
        link.backward = Box::new(move |value| {
            let value =
                U::from_reflect(value).ok_or_else(|| anyhow!("Invalid two-way target type"))?;
            Ok(Box::new(backward(value)?))
        });
        self.links.push(link);
        self
    }

    /// Install once, in the same World where this graph will be evaluated.
    pub fn install(mut self, world: &mut World, owner: Entity) -> Result<()> {
        ensure!(
            world.get_entity(owner).is_ok(),
            "Binding graph owner is missing"
        );
        for node in &mut self.nodes {
            node.system.initialize(world);
            ensure!(
                !node.system.has_deferred(),
                "{}: Binding computations cannot contain deferred writes (Commands)",
                node.name
            );
            ensure!(
                node.system.is_send(),
                "{}: Binding computations must use Send resources",
                node.name
            );
        }
        for sink in &mut self.sinks {
            sink.read.initialize(world);
            sink.system.initialize(world);
        }
        for link in &mut self.links {
            link.initialize(world);
        }
        world.init_resource::<BindingGraphs>();
        world
            .resource_mut::<BindingGraphs>()
            .graphs
            .push(OwnedGraph {
                owner,
                graph: self,
                error: None,
            });
        Ok(())
    }

    fn evaluate(&mut self, world: &mut World) -> Result<()> {
        let mut values: Vec<BindingValue> = Vec::with_capacity(self.nodes.len());
        for node in &mut self.nodes {
            node.system.check_change_tick(world.read_change_tick());
            node.system
                .validate_param(world)
                .map_err(|error| anyhow!("{}: {error}", node.name))?;
            let inputs = node
                .inputs
                .iter()
                .map(|index| copy_value(&values[*index]))
                .collect();
            values.push(
                node.system
                    .run_readonly(inputs, world)
                    .map_err(|error| anyhow!("{}: {error}", node.name))?,
            );
        }
        // Plan all link directions against the same pre-write world.
        let plans = self
            .links
            .iter_mut()
            .map(|link| link.plan(world))
            .collect::<Result<Vec<_>>>()?;
        for sink in &mut self.sinks {
            sink.read.check_change_tick(world.read_change_tick());
            sink.read
                .validate_param(world)
                .map_err(|error| anyhow!("Binding destination: {error}"))?;
            let current = sink.read.run_readonly(vec![], world)?;
            if values[sink.input].is_none() || equal(&current, &values[sink.input]) == Some(true) {
                continue;
            }
            sink.system.check_change_tick(world.read_change_tick());
            sink.system
                .validate_param(world)
                .map_err(|error| anyhow!("Binding destination: {error}"))?;
            sink.system.run(copy_value(&values[sink.input]), world)?;
        }
        for (link, plan) in self.links.iter_mut().zip(plans) {
            link.commit(plan, world)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum BindingConflict {
    /// Both sides changed differently since the last successful synchronization.
    #[default]
    Reject,
    SourceWins,
    TargetWins,
}

type Conversion = Box<dyn Fn(&dyn PartialReflect) -> Result<Box<dyn PartialReflect>> + Send + Sync>;

struct TwoWayLink {
    forward: Conversion,
    backward: Conversion,
    source: Reader,
    target: Reader,
    source_route: Reader,
    target_route: Reader,
    route: Option<(BindingValue, BindingValue)>,
    write_source: Writer,
    write_target: Writer,
    baseline: Option<(BindingValue, BindingValue)>,
    conflict: BindingConflict,
}

enum LinkPlan {
    Missing,
    Keep,
    Source(BindingValue),
    Target(BindingValue),
}

impl TwoWayLink {
    fn new(source: BindingPath, target: BindingPath, conflict: BindingConflict) -> Self {
        Self {
            source_route: source.route_reader(),
            target_route: target.route_reader(),
            route: None,
            source: source.reader(),
            target: target.reader(),
            write_source: source.writer(),
            write_target: target.writer(),
            baseline: None,
            conflict,
            forward: Box::new(|value| Ok(value.clone_value())),
            backward: Box::new(|value| Ok(value.clone_value())),
        }
    }
    fn initialize(&mut self, world: &mut World) {
        self.source.initialize(world);
        self.target.initialize(world);
        self.source_route.initialize(world);
        self.target_route.initialize(world);
        self.write_source.initialize(world);
        self.write_target.initialize(world);
    }
    fn read(&mut self, world: &World) -> Result<(BindingValue, BindingValue)> {
        for reader in [&mut self.source, &mut self.target] {
            reader.check_change_tick(world.read_change_tick());
            reader
                .validate_param(world)
                .map_err(|error| anyhow!("Two-way binding: {error}"))?;
        }
        Ok((
            self.source.run_readonly(vec![], world)?,
            self.target.run_readonly(vec![], world)?,
        ))
    }
    fn plan(&mut self, world: &World) -> Result<LinkPlan> {
        let (source, target) = self.read(world)?;
        self.source_route
            .check_change_tick(world.read_change_tick());
        self.target_route
            .check_change_tick(world.read_change_tick());
        let route = (
            self.source_route.run_readonly(vec![], world)?,
            self.target_route.run_readonly(vec![], world)?,
        );
        if self.route.as_ref().is_none_or(|old| {
            equal(&old.0, &route.0) != Some(true) || equal(&old.1, &route.1) != Some(true)
        }) {
            self.baseline = None;
        }
        self.route = Some(route);
        if source.is_none() || target.is_none() {
            self.baseline = None;
            return Ok(LinkPlan::Missing);
        }
        let same = |a: &BindingValue, b: &BindingValue| {
            equal(a, b).ok_or_else(|| anyhow!("Two-way bindings require reflected equality"))
        };
        // Equality must be supported within each endpoint's type, even when
        // source and target intentionally have different types.
        same(&source, &source)?;
        same(&target, &target)?;
        let projected = Some((self.forward)(source.as_ref().unwrap().as_ref())?);
        if same(&projected, &target)? {
            return Ok(LinkPlan::Keep);
        }
        let Some((old_source, old_target)) = &self.baseline else {
            return Ok(LinkPlan::Source(projected));
        };
        match (!same(&source, old_source)?, !same(&target, old_target)?) {
            (true, false) => Ok(LinkPlan::Source(projected)),
            (false, true) => Ok(LinkPlan::Target(Some((self.backward)(
                target.as_ref().unwrap().as_ref(),
            )?))),
            (false, false) => Ok(LinkPlan::Keep),
            (true, true) => match self.conflict {
                BindingConflict::Reject => {
                    Err(anyhow!("Two-way binding conflict: both endpoints changed"))
                }
                BindingConflict::SourceWins => Ok(LinkPlan::Source(projected)),
                BindingConflict::TargetWins => Ok(LinkPlan::Target(Some((self.backward)(
                    target.as_ref().unwrap().as_ref(),
                )?))),
            },
        }
    }
    fn commit(&mut self, plan: LinkPlan, world: &mut World) -> Result<()> {
        match plan {
            LinkPlan::Missing => return Ok(()),
            LinkPlan::Keep => {}
            LinkPlan::Source(value) => {
                self.write_target.run(value, world)?;
            }
            LinkPlan::Target(value) => {
                self.write_source.run(value, world)?;
            }
        }
        // Store actual post-conversion values, avoiding echo writes next frame.
        self.baseline = Some(self.read(world)?);
        Ok(())
    }
}

/// Install graphs from UI builders or ordinary Bevy systems.
pub trait BindingGraphCommandsExt {
    fn bind_graph(&mut self, owner: Entity, graph: BindingGraph);
}
impl BindingGraphCommandsExt for Commands<'_, '_> {
    fn bind_graph(&mut self, owner: Entity, graph: BindingGraph) {
        self.queue(move |world: &mut World| -> bevy::prelude::Result {
            graph.install(world, owner).map_err(Into::into)
        });
    }
}

struct OwnedGraph {
    owner: Entity,
    graph: BindingGraph,
    error: Option<String>,
}

#[derive(Resource, Default)]
pub struct BindingGraphs {
    graphs: Vec<OwnedGraph>,
}

impl BindingGraphs {
    pub fn errors(&self) -> impl Iterator<Item = (Entity, &str)> {
        self.graphs
            .iter()
            .filter_map(|graph| graph.error.as_deref().map(|error| (graph.owner, error)))
    }
    pub fn remove_owner(&mut self, owner: Entity) {
        self.graphs.retain(|graph| graph.owner != owner);
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingGraphSet;

/// Opt-in so existing apps can choose ordering relative to legacy bindings/UI.
/// Computes once in PostUpdate. No fixpoint loop, implicit feedback, or DB I/O.
pub struct BindingGraphPlugin;
impl Plugin for BindingGraphPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BindingGraphs>()
            .add_systems(PostUpdate, evaluate_binding_graphs.in_set(BindingGraphSet));
    }
}

pub fn evaluate_binding_graphs(world: &mut World) {
    world.resource_scope(|world, mut graphs: Mut<BindingGraphs>| {
        graphs
            .graphs
            .retain(|graph| world.get_entity(graph.owner).is_ok());
        for graph in &mut graphs.graphs {
            let error = graph
                .graph
                .evaluate(world)
                .err()
                .map(|error| error.to_string());
            if error != graph.error {
                if let Some(error) = &error {
                    tracing::warn!(owner = ?graph.owner, %error, "Binding graph failed");
                }
            }
            graph.error = error;
        }
    });
}

#[cfg(test)]
mod tests;

mod paths;
pub use paths::*;

pub mod editing;
pub use editing::*;
