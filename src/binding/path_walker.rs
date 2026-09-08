use std::slice;

use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use bevy_reflect::ReflectRef;
use bevy_trait_query::All;

use crate::prelude::*;

/// Abstracts over "how do I look up a reactive component" so the path walker
/// isn't tied to any specific Query/DBConfig pair.
pub trait EntityResolver {
    fn get_reactive(&self, entity: Entity, component_name: &str) -> Option<Box<dyn PartialReflect>>;
    fn resolve_id(&self, id: &Id) -> Option<Entity>;
}

/// The default resolver, usable anywhere you have a reactives query + DBConfig.
pub struct ReactiveResolver<'a, 'w, 's> {
    pub reactives: &'a Query<'w, 's, (Entity, All<&'static mut dyn Reactive>)>,
    pub db_config: &'a DBConfig,
}

impl<'a, 'w, 's> EntityResolver for ReactiveResolver<'a, 'w, 's> {
    fn get_reactive(&self, entity: Entity, component_name: &str) -> Option<Box<dyn PartialReflect>> {
        let (_, reactives) = self.reactives.get(entity).ok()?;
        reactives
            .iter()
            .find(|x| x.reflect_short_type_path() == component_name)
            .map(|r| r.clone_value())
    }

    fn resolve_id(&self, id: &Id) -> Option<Entity> {
        self.db_config.get_entity(id)
    }
}

/// One step taken while walking a property path.
#[derive(Debug)]
pub enum PathStep {
    /// A plain field/index access — no entity jump.
    Field {
        access: OffsetAccess,
        value: Box<dyn PartialReflect>,
    },
    /// The current value was an `Id`, which resolved to a different entity + component.
    EntityJump {
        access: OffsetAccess,
        entity: Entity,
        component_name: String,
    },
}

/// Why a walk stopped before exhausting the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathWalkStop {
    OptionWasNone,
    EntityMissing,
    ComponentMissing(String),
    AccessError,
}

pub struct PathWalker<'a, R: EntityResolver> {
    resolver: &'a R,
    current: Box<dyn PartialReflect>,
    remaining: std::slice::Iter<'a, OffsetAccess>,
    stopped: Option<PathWalkStop>,
}

impl<'a, R: EntityResolver> PathWalker<'a, R> {
    pub fn new(root: Box<dyn PartialReflect>, path: &'a OptionalParsedPath, resolver: &'a R) -> Self {
        Self {
            resolver,
            current: root,
            remaining: path.0.iter(),
            stopped: None,
        }
    }

    /// Set if the walk ended early rather than just running out of path.
    pub fn stop_reason(&self) -> Option<&PathWalkStop> {
        self.stopped.as_ref()
    }

    /// The last value reached, whether or not the walk fully completed.
    pub fn current_value(&self) -> &dyn PartialReflect {
        self.current.as_ref()
    }
}

impl<'a, R: EntityResolver> Iterator for PathWalker<'a, R> {
    type Item = PathStep;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stopped.is_some() {
            return None;
        }

        let offset_access = self.remaining.next()?;

        // Unwrap Option<T> layers before interpreting this access.
        let (is_option, inner) = get_inner_if_option(&self.current);
        if is_option {
            match inner {
                Some(inner_value) => self.current = inner_value,
                None => {
                    self.stopped = Some(PathWalkStop::OptionWasNone);
                    return None;
                }
            }
        }

        // If the current value is an `Id`, this access jumps to another entity.
        if let Some(id) = Id::from_reflect(self.current.as_partial_reflect()) {
            let component_name = offset_access.access.display_value().to_string();

            let Some(entity) = self.resolver.resolve_id(&id) else {
                self.stopped = Some(PathWalkStop::EntityMissing);
                return None;
            };
            let Some(value) = self.resolver.get_reactive(entity, &component_name) else {
                self.stopped = Some(PathWalkStop::ComponentMissing(component_name.clone()));
                return Some(PathStep::EntityJump {
                    access: offset_access.clone(),
                    entity,
                    component_name,
                });
            };

            self.current = value;
            return Some(PathStep::EntityJump {
                access: offset_access.clone(),
                entity,
                component_name,
            });
        }

        // Otherwise, a normal field/index access on the current value.
        match offset_access.access.element(self.current.as_ref(), offset_access.offset) {
            Ok(child) => {
                self.current = child.clone_value();
                Some(PathStep::Field {
                    access: offset_access.clone(),
                    value: self.current.clone_value(),
                })
            }
            Err(_) => {
                self.stopped = Some(PathWalkStop::AccessError);
                None
            }
        }
    }
}

pub fn get_inner_if_option(value: &Box<dyn PartialReflect>) -> (bool, Option<Box<dyn PartialReflect>>) {
    let is_option = is_option(value);
    if is_option {
        if let ReflectRef::Enum(dyn_enum) = value.reflect_ref() {
            if let Some(inner) = dyn_enum.field_at(0) {
                return (true, Some(inner.clone_value()));
            }
        }
        return (true, None);
    }
    (false, None)
}

pub fn is_option(value: &Box<dyn PartialReflect>) -> bool {
    if let Some(info) = value.get_represented_type_info() {
        info.type_path_table().short_path().starts_with("Option<")
    } else {
        value.reflect_short_type_path().starts_with("Option<")
    }
}