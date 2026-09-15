//! Explicit display, live, and draft editing of typed values behind checked paths.
use super::*;
use crate::prelude::{EntitySys, IntoResult, ReactiveView};
use bevy_trait_query::RegisterExt;
use std::collections::HashMap;

/// UI write policy. Display-only rejects edits through the session API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditPolicy {
    DisplayOnly,
    Live,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    ReadOnly,
    NotEditing,
    Missing,
    SourceChanged,
    CollectionChanged,
    DuplicateKey,
    Invalid,
    WriteFailed,
}
impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ReadOnly => "This value is read-only.",
            Self::NotEditing => "Begin editing first.",
            Self::Missing => "The original item is unavailable.",
            Self::SourceChanged => "The original item changed. Cancel to reload it.",
            Self::CollectionChanged => "The collection changed. Cancel to reload it.",
            Self::DuplicateKey => "The collection contains duplicate item keys.",
            Self::Invalid => "The edited value is invalid.",
            Self::WriteFailed => "The edit could not be applied.",
        })
    }
}
impl std::error::Error for EditError {}

/// Reactive presentation state. Error messages deliberately contain no values.
#[derive(Component, Reflect, Clone, Default, PartialEq)]
pub struct EditStatus {
    pub active: bool,
    pub viewing: bool,
    pub stale: bool,
    pub missing: bool,
    pub message: String,
    /// Counts successful local commits, not persistence or transport acknowledgements.
    pub commits: u64,
}
impl Reactive for EditStatus {}

/// Private draft storage prevents a display binding from exposing a writable draft.
/// UI callbacks call begin/edit/save/cancel; application systems may inspect value/status.
#[derive(Component, Clone)]
pub struct EditSession<T: Send + Sync + 'static> {
    policy: EditPolicy,
    value: T,
    baseline: T,
    latest: Option<T>,
    active: bool,
    save: bool,
    cancel: bool,
    error: Option<EditError>,
    commits: u64,
    epoch: u64,
    invalid_inputs: HashSet<Entity>,
}
impl<T: Clone + Send + Sync + 'static> EditSession<T> {
    fn new(value: T, policy: EditPolicy) -> Self {
        Self {
            baseline: value.clone(),
            latest: Some(value.clone()),
            value,
            policy,
            active: false,
            save: false,
            cancel: false,
            error: None,
            commits: 0,
            epoch: 0,
            invalid_inputs: HashSet::new(),
        }
    }
    pub fn value(&self) -> &T {
        &self.value
    }
    /// Most recently observed source value, kept separate from the visible draft.
    pub fn latest(&self) -> Option<&T> {
        self.latest.as_ref()
    }
    pub fn baseline(&self) -> &T {
        &self.baseline
    }
    pub fn policy(&self) -> EditPolicy {
        self.policy
    }
    pub fn is_editing(&self) -> bool {
        self.active
    }
    pub fn error(&self) -> Option<EditError> {
        self.error
    }
    pub fn commits(&self) -> u64 {
        self.commits
    }
    pub fn begin(&mut self) -> std::result::Result<(), EditError> {
        if self.policy == EditPolicy::DisplayOnly {
            return Err(EditError::ReadOnly);
        }
        if !self.active {
            self.baseline = self.value.clone();
            self.epoch += 1;
            self.invalid_inputs.clear();
            self.active = true;
        }
        Ok(())
    }
    pub fn edit(&mut self, edit: impl FnOnce(&mut T)) -> std::result::Result<(), EditError> {
        if self.policy == EditPolicy::DisplayOnly {
            return Err(EditError::ReadOnly);
        }
        if !self.active && self.policy == EditPolicy::Live {
            self.begin()?;
        }
        if !self.active {
            return Err(EditError::NotEditing);
        }
        edit(&mut self.value);
        if self.policy == EditPolicy::Live {
            self.save = true;
        }
        Ok(())
    }
    pub fn save(&mut self) -> std::result::Result<(), EditError> {
        if self.policy == EditPolicy::DisplayOnly {
            return Err(EditError::ReadOnly);
        }
        if !self.active {
            return Err(EditError::NotEditing);
        }
        self.save = true;
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.cancel = true;
        self.save = false;
    }
}

mod adapters;
pub use adapters::*;

/// Owns the initialized row-construction system without permanent SystemId registrations.
/// Deferred row construction executes with the editor registry available.
pub struct EditRenderer(
    std::sync::Arc<std::sync::Mutex<Box<dyn FnMut(Entity, &mut World) -> Result<()> + Send>>>,
);
impl EditRenderer {
    pub fn new<S, SM>(system: S) -> Self
    where
        S: EntitySys<SM>,
    {
        let mut system = IntoSystem::into_system(system);
        let mut initialized = false;
        Self(std::sync::Arc::new(std::sync::Mutex::new(Box::new(
            move |entity, world| {
                if !initialized {
                    system.initialize(world);
                    initialized = true;
                }
                system.check_change_tick(world.read_change_tick());
                system
                    .validate_param(world)
                    .map_err(|_| anyhow!("Row renderer parameters unavailable"))?;
                let result = system.run(entity, world).into_entity_result();
                system.apply_deferred(world);
                result.map_err(|_| anyhow!("Row renderer failed"))
            },
        ))))
    }
    fn call(&self, commands: &mut Commands, entity: Entity) {
        let renderer = self.0.clone();
        commands.queue(move |world: &mut World| -> bevy::prelude::Result {
            if world.get_entity(entity).is_ok() {
                renderer
                    .lock()
                    .map_err(|_| anyhow!("Row renderer unavailable"))?(
                    entity, world
                )?;
            }
            Ok(())
        });
    }
}

struct Row<C> {
    entity: Entity,
    snapshot: std::sync::Arc<C>,
    route: Vec<u64>,
}
type Validator<T> = Box<dyn Fn(&T) -> bool + Send + Sync>;
trait Driver: Send + Sync {
    fn update(&mut self, world: &mut World) -> Result<()>;
}
struct CollectionDriver<A: EditCollection> {
    owner: Entity,
    adapter: A,
    policy: EditPolicy,
    reader: Reader,
    route_reader: Reader,
    writer: Writer,
    rows: HashMap<A::Key, Row<A::Collection>>,
    render: EditRenderer,
    validate: Validator<A::Item>,
}

/// Triggered on the row after a successful local commit. Observe this event to
/// initiate domain-owned persistence or transport actions; no payload values are logged.
#[derive(Event, Clone, Copy, Debug)]
pub struct EditCommitted {
    pub owner: Entity,
    pub row: Entity,
    pub sequence: u64,
}

#[derive(Component)]
struct EditBindingOwner;
#[derive(Component)]
struct EditInputOwner;

/// Installed editable collections. Owners scope lifetime; dropping an owner releases its driver.
#[derive(Resource, Default)]
pub struct EditBindings {
    drivers: HashMap<Entity, Box<dyn Driver>>,
    inputs: HashMap<Entity, Box<dyn InputDriver>>,
}
#[derive(SystemSet, Clone, Debug, Hash, PartialEq, Eq)]
pub struct EditBindingSet;
pub struct EditBindingPlugin;
impl Plugin for EditBindingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditBindings>()
            .register_component_as::<dyn Reactive, EditStatus>()
            .register_component_as::<dyn Reactive, EditInputStatus>()
            .add_systems(Update, update_edit_bindings.in_set(EditBindingSet));
    }
}
fn update_edit_bindings(world: &mut World) {
    // Missing DBConfig means paths are not available yet, not a lost draft.
    if !world.contains_resource::<DBConfig>() {
        return;
    }
    // World operations can flush queued renderers which install nested editors.
    // Keep the registry present while running drivers, protected by owner markers.
    let (mut drivers, mut inputs) = {
        let mut registry = world.resource_mut::<EditBindings>();
        (
            std::mem::take(&mut registry.drivers),
            std::mem::take(&mut registry.inputs),
        )
    };
    inputs.retain(|input, _| world.get_entity(*input).is_ok());
    for input in inputs.values_mut() {
        input.pull(world);
    }
    drivers.retain(|owner, _| world.get_entity(*owner).is_ok());
    let mut owners: Vec<Entity> = drivers.keys().copied().collect();
    owners.sort_by_key(|owner| owner.to_bits());
    for owner in owners {
        if drivers.get_mut(&owner).unwrap().update(world).is_err() {
            if let Some(mut status) = world.get_mut::<EditStatus>(owner) {
                status.message = EditError::WriteFailed.to_string();
            }
        }
    }
    for input in inputs.values_mut() {
        input.push(world);
    }
    let mut registry = world.resource_mut::<EditBindings>();
    registry.drivers.extend(drivers);
    registry.inputs.extend(inputs);
}

/// Install on a list/container entity. Renderer callbacks run once per row identity.
/// `ReactiveView` is supplied for struct items as a read-only presentation snapshot;
/// edits must go through `EditSession<T>`, never back through that snapshot.
pub fn install_edit_binding<A: EditCollection>(
    world: &mut World,
    owner: Entity,
    path: BindingPath,
    adapter: A,
    policy: EditPolicy,
    render: EditRenderer,
    validate: impl Fn(&A::Item) -> bool + Send + Sync + 'static,
) -> Result<()> {
    ensure!(
        world.contains_resource::<EditBindings>(),
        "Add EditBindingPlugin before installing editable bindings"
    );
    ensure!(
        world.get::<EditBindingOwner>(owner).is_none(),
        "An edit binding already owns this entity"
    );
    ensure!(
        world
            .get::<crate::prelude::ReactiveListView>(owner)
            .is_none(),
        "Legacy and editable renderers cannot share an owner"
    );
    let mut reader = path.reader();
    let mut route_reader = path.route_reader();
    let mut writer = path.writer();
    reader.initialize(world);
    route_reader.initialize(world);
    writer.initialize(world);
    world
        .entity_mut(owner)
        .insert((EditStatus::default(), EditBindingOwner));
    world.resource_mut::<EditBindings>().drivers.insert(
        owner,
        Box::new(CollectionDriver {
            owner,
            adapter,
            policy,
            reader,
            route_reader,
            writer,
            rows: HashMap::new(),
            render,
            validate: Box::new(validate),
        }),
    );
    Ok(())
}

fn present<T: Reflect + FromReflect + Clone + PartialEq>(
    world: &mut World,
    entity: Entity,
    value: &T,
) {
    if let ReflectRef::Struct(s) = value.reflect_ref() {
        let same = world
            .get::<ReactiveView>(entity)
            .and_then(|v| T::from_reflect(&v.value))
            .is_some_and(|old| old == *value);
        if !same {
            world.entity_mut(entity).insert(ReactiveView {
                value: s.to_dynamic_struct(),
            });
        }
    }
}

impl<A: EditCollection> Driver for CollectionDriver<A> {
    fn update(&mut self, world: &mut World) -> Result<()> {
        self.reader.check_change_tick(world.read_change_tick());
        self.route_reader
            .check_change_tick(world.read_change_tick());
        self.reader
            .validate_param(world)
            .map_err(|_| anyhow!("Edit source unavailable"))?;
        let value = self.reader.run_readonly(vec![], world)?;
        let mut collection = value
            .as_ref()
            .and_then(|v| A::Collection::from_reflect(v.as_ref()));
        let mut route = self
            .route_reader
            .run_readonly(vec![], world)?
            .as_ref()
            .and_then(|v| Vec::<u64>::from_reflect(v.as_ref()));
        let entries = collection
            .as_ref()
            .map(|c| self.adapter.entries(c))
            .unwrap_or_default();
        let mut keys = HashSet::new();
        let duplicate = entries.iter().any(|(key, _)| !keys.insert(key.clone()));
        let mut owner_status = EditStatus {
            viewing: true,
            missing: collection.is_none() || route.is_none(),
            ..default()
        };
        if duplicate {
            owner_status.message = EditError::DuplicateKey.to_string();
        } else if owner_status.missing {
            owner_status.message = if value.is_some() && collection.is_none() {
                EditError::Invalid
            } else {
                EditError::Missing
            }
            .to_string();
        }
        if world.get::<EditStatus>(self.owner) != Some(&owner_status) {
            world.entity_mut(self.owner).insert(owner_status);
        }
        let mut latest: HashMap<A::Key, A::Item> = entries.iter().cloned().collect();
        let mut snapshot = collection.as_ref().map(|c| std::sync::Arc::new(c.clone()));
        let mut order = Vec::new();
        if !duplicate {
            if let (Some(_), Some(r)) = (&collection, &route) {
                for (key, item) in &entries {
                    if let Some(row) = self.rows.get(key) {
                        if world.get_entity(row.entity).is_err() {
                            self.rows.remove(key);
                        }
                    }
                    let row = self.rows.entry(key.clone()).or_insert_with(|| {
                        let entity = world
                            .spawn((
                                EditSession::new(item.clone(), self.policy),
                                EditStatus::default(),
                            ))
                            .id();
                        world.entity_mut(self.owner).add_child(entity);
                        present(world, entity, item);
                        self.render.call(&mut world.commands(), entity);
                        Row {
                            entity,
                            snapshot: snapshot.as_ref().unwrap().clone(),
                            route: r.clone(),
                        }
                    });
                    order.push(row.entity);
                }
            }
        }
        let mut notifications = Vec::new();
        let mut remove = Vec::new();
        let mut row_keys: Vec<_> = self.rows.keys().cloned().collect();
        row_keys.sort_by_key(|key| self.rows[key].entity.to_bits());
        for key in &row_keys {
            let row = self.rows.get_mut(key).unwrap();
            let mut committed = false;
            world.flush();
            let Some(mut session) = world.get::<EditSession<A::Item>>(row.entity).cloned() else {
                remove.push(key.clone());
                continue;
            };
            let current = if duplicate {
                None
            } else {
                latest
                    .get(key)
                    .cloned()
                    .or_else(|| collection.as_ref().and_then(|c| self.adapter.get(c, key)))
            };
            session.latest = current.clone();
            session
                .invalid_inputs
                .retain(|input| world.get_entity(*input).is_ok());

            if session.cancel {
                session.active = false;
                session.epoch += 1;
                session.invalid_inputs.clear();
                session.cancel = false;
                session.save = false;
                session.error = None;
            }
            if !session.active && !latest.contains_key(key) {
                world.despawn(row.entity);
                remove.push(key.clone());
                continue;
            }
            if !session.active {
                if let (Some(item), Some(_), Some(r)) = (&current, &collection, &route) {
                    session.value = item.clone();
                    session.baseline = item.clone();
                    row.snapshot = snapshot.as_ref().unwrap().clone();
                    row.route = r.clone();
                } else {
                    world.despawn(row.entity);
                    remove.push(key.clone());
                    continue;
                }
            } else if session.save {
                session.save = false;
                let result = (|| -> std::result::Result<(), EditError> {
                    // Queued renderers/commit observers may have changed the model.
                    // Re-read immediately before validation and writing, never replace
                    // a collection captured before those application actions ran.
                    let fresh = self
                        .reader
                        .run_readonly(vec![], world)
                        .map_err(|_| EditError::Missing)?;
                    let fresh = fresh
                        .as_ref()
                        .and_then(|v| A::Collection::from_reflect(v.as_ref()))
                        .ok_or(EditError::Missing)?;
                    route = self
                        .route_reader
                        .run_readonly(vec![], world)
                        .map_err(|_| EditError::Missing)?
                        .as_ref()
                        .and_then(|v| Vec::<u64>::from_reflect(v.as_ref()));
                    let fresh_entries = self.adapter.entries(&fresh);
                    let mut fresh_keys = HashSet::new();
                    if fresh_entries
                        .iter()
                        .any(|(k, _)| !fresh_keys.insert(k.clone()))
                    {
                        return Err(EditError::DuplicateKey);
                    }
                    session.latest = self.adapter.get(&fresh, key);
                    let item = session.latest.as_ref().ok_or(EditError::Missing)?;
                    if route.as_ref() != Some(&row.route) {
                        return Err(EditError::SourceChanged);
                    }
                    if self.adapter.guard_collection() && fresh != *row.snapshot {
                        return Err(EditError::CollectionChanged);
                    }
                    snapshot = Some(std::sync::Arc::new(fresh.clone()));
                    collection = Some(fresh);
                    latest = fresh_entries.into_iter().collect();
                    if item != &session.baseline && item != &session.value {
                        return Err(EditError::SourceChanged);
                    }
                    if !session.invalid_inputs.is_empty() || !(self.validate)(&session.value) {
                        return Err(EditError::Invalid);
                    }
                    if item != &session.value {
                        let mut next = collection.as_ref().ok_or(EditError::Missing)?.clone();
                        self.adapter
                            .replace(&mut next, key, session.value.clone())?;
                        self.writer.check_change_tick(world.read_change_tick());
                        self.writer
                            .validate_param(world)
                            .map_err(|_| EditError::WriteFailed)?;
                        self.writer
                            .run(Some(Box::new(next.clone())), world)
                            .map_err(|_| EditError::WriteFailed)?;
                        snapshot = Some(std::sync::Arc::new(next.clone()));
                        collection = Some(next);
                        latest.insert(key.clone(), session.value.clone());
                    }
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        session.baseline = session.value.clone();
                        session.error = None;
                        session.commits += 1;
                        row.snapshot = snapshot.as_ref().unwrap().clone();
                        session.latest = Some(session.value.clone());
                        committed = true;
                        session.active = false;
                    }
                    Err(error) => session.error = Some(error),
                }
            }
            let stale = session.latest.as_ref() != Some(&session.baseline)
                || route.as_ref() != Some(&row.route)
                || (self.adapter.guard_collection()
                    && collection.as_ref() != Some(row.snapshot.as_ref()));
            let status = EditStatus {
                active: session.active,
                viewing: !session.active,
                stale: session.active && stale,
                missing: current.is_none(),
                message: session.error.map(|e| e.to_string()).unwrap_or_else(|| {
                    if session.active && stale {
                        "The source changed while editing.".into()
                    } else {
                        String::new()
                    }
                }),
                commits: session.commits,
            };
            present(world, row.entity, &session.value);
            if world.get::<EditStatus>(row.entity) != Some(&status) {
                world.entity_mut(row.entity).insert(status);
            }
            let sequence = session.commits;
            *world.get_mut::<EditSession<A::Item>>(row.entity).unwrap() = session;
            if committed {
                notifications.push((row.entity, sequence));
            }
            if !order.contains(&row.entity) {
                order.push(row.entity);
            }
        }
        for key in remove {
            self.rows.remove(&key);
        }
        order.retain(|e| world.get_entity(*e).is_ok());
        // Only this adapter's rows are reordered; unrelated children are preserved.
        let mut children: Vec<Entity> = world
            .get::<Children>(self.owner)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        children.retain(|e| !self.rows.values().any(|r| r.entity == *e));
        if !self.adapter.ordered() {
            let existing: Vec<Entity> = world
                .get::<Children>(self.owner)
                .map(|c| c.iter().collect())
                .unwrap_or_default();
            let mut stable: Vec<Entity> =
                existing.into_iter().filter(|e| order.contains(e)).collect();
            for e in order {
                if !stable.contains(&e) {
                    stable.push(e);
                }
            }
            order = stable;
        }
        children.extend(order);
        let existing: Vec<Entity> = world
            .get::<Children>(self.owner)
            .map(|c| c.iter().collect())
            .unwrap_or_default();
        if existing != children {
            world.entity_mut(self.owner).replace_children(&children);
        }
        for (row, sequence) in notifications {
            world.commands().trigger_targets(
                EditCommitted {
                    owner: self.owner,
                    row,
                    sequence,
                },
                row,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

mod inputs;
use inputs::InputDriver;
pub use inputs::*;
