use super::*;

/// A component/property location without an entity; useful for builder targets.
///
/// ```
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Example { value: i32 }
/// let path = binding_path!(Entity::PLACEHOLDER, Example.value).unwrap();
/// let target = component_path!(Example.value).unwrap().at(Entity::PLACEHOLDER);
/// assert_eq!(path, target);
/// assert_eq!(property_path!(Example.value), "value");
/// ```
///
/// Misspelled fields are compiler errors:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Example { value: i32 }
/// let path = binding_path!(Entity::PLACEHOLDER, Example.valeu);
/// ```
///
/// Entity jumps require Id values, not arbitrary fields:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Example { value: i32 }
/// let path = binding_path!(Entity::PLACEHOLDER, Example.value -> Example.value);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentBindingPath {
    component: String,
    path: Option<String>,
    parsed: OptionalParsedPath,
}
impl ComponentBindingPath {
    pub fn new(component: impl Into<String>, path: Option<&str>) -> Result<Self> {
        Ok(Self {
            component: component.into(),
            path: path.map(str::to_owned),
            parsed: OptionalParsedPath::parse(path.unwrap_or(""))
                .map_err(|error| anyhow!("Invalid binding path: {error}"))?,
        })
    }
    pub fn at(self, entity: Entity) -> BindingPath {
        BindingPath {
            entity,
            component: self.component,
            path: self.path,
            parsed: self.parsed,
        }
    }
}

impl BindingGraph {
    /// Compile a graph containing only direct source-to-destination edges into
    /// the existing change-driven scheduler. This preserves editable UI values,
    /// cascading updates, Binding inspection, and list callbacks.
    /// Computations and two-way links require the explicit graph runtime instead.
    pub fn into_bindings(self) -> Result<Vec<crate::binding::Binding>> {
        ensure!(
            self.links.is_empty() && self.nodes.iter().all(|node| node.source_path.is_some()),
            "Only direct path graphs support compatibility scheduling; use BindingGraphPlugin for computations or two-way links"
        );
        self.sinks
            .into_iter()
            .map(|sink| {
                let source = self.nodes[sink.input].source_path.as_ref().unwrap();
                let target = sink.target_path;
                Ok(crate::binding::Binding {
                    source_entity: Some(source.entity),
                    source_component_name: source.component.clone(),
                    source_property_path: source.path.clone(),
                    target_entity: Some(target.entity),
                    target_component_name: target.component,
                    target_property_path: target.path,
                    entity_func: None,
                })
            })
            .collect()
    }
}

/// Defer installation through the same FluxWorld command boundary as the
/// original builders. `None` remains an inactive, inspectable Binding source.
#[cfg(feature = "bevy_std")]
pub(crate) fn queue_builder_binding(
    commands: &mut Commands,
    source: Result<BindingPath>,
    target: Result<BindingPath>,
) {
    use bevy::ecs::system::RunSystemOnce;
    commands.queue(move |world: &mut World| -> bevy::prelude::Result {
        let mut graph = BindingGraph::new();
        let source = source?;
        let source_entity = source.entity;
        let source = graph.source(source)?;
        graph.bind(source, target?)?;
        let mut edges = graph.into_bindings()?;
        for edge in &mut edges {
            edge.source_entity = Some(source_entity);
        }
        world.run_system_once(move |mut bindings: crate::binding::FluxWorld| {
            for edge in &edges {
                bindings.add_binding(edge.clone());
            }
        })?;
        Ok(())
    });
}

#[cfg(feature = "bevy_std")]
pub(crate) fn queue_checked_builder_binding(
    commands: &mut Commands,
    source: BindingPath,
    target: BindingPath,
) {
    queue_builder_binding(
        commands,
        Ok(source),
        Ok(target),
    );
}

/// Builder input: accept both an already checked location and a macro's Result.
/// Builders report invalid paths through Bevy's command error handler.
pub trait IntoBindingPath {
    fn into_binding_path(self) -> Result<BindingPath>;
}
impl IntoBindingPath for BindingPath {
    fn into_binding_path(self) -> Result<BindingPath> {
        Ok(self)
    }
}
impl IntoBindingPath for Result<BindingPath> {
    fn into_binding_path(self) -> Result<BindingPath> {
        self
    }
}
pub trait IntoComponentBindingPath {
    fn into_component_binding_path(self) -> Result<ComponentBindingPath>;
}
impl IntoComponentBindingPath for ComponentBindingPath {
    fn into_component_binding_path(self) -> Result<ComponentBindingPath> {
        Ok(self)
    }
}
impl IntoComponentBindingPath for Result<ComponentBindingPath> {
    fn into_component_binding_path(self) -> Result<ComponentBindingPath> {
        self
    }
}
