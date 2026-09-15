use super::*;

/// A component/property location without an entity; useful for builder targets.
///
/// ```
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Example { value: i32 }
/// let path = path!(Entity::PLACEHOLDER, Example.value).unwrap();
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
/// let path = path!(Entity::PLACEHOLDER, Example.valeu);
/// ```
///
/// Entity jumps require Id values, not arbitrary fields:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Example { value: i32 }
/// let path = path!(Entity::PLACEHOLDER, Example.value -> Example.value);
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


/// String-based builder conveniences use the same graph preparation and
/// installation path as expression-based builders.
#[cfg(feature = "bevy_std")]
pub(crate) fn queue_builder_binding(
    commands: &mut Commands,
    source: Result<BindingPath>,
    target: Result<BindingPath>,
) {
    commands.queue(move |world: &mut World| -> bevy::prelude::Result {
        let target = target?;
        let owner = target.entity;
        source.into_binding_expr().prepare(target)?.install(world, owner)?;
        Ok(())
    });
}

/// Builder input: accept both an already checked location and a macro's Result.
/// Builders report invalid paths through Bevy's command error handler.
pub trait IntoBindingPath {
    type Value;
    fn into_binding_path(self) -> Result<BindingPath>;
}
impl IntoBindingPath for BindingPath {
    type Value = Untyped;
    fn into_binding_path(self) -> Result<BindingPath> {
        Ok(self)
    }
}
impl IntoBindingPath for Result<BindingPath> {
    type Value = Untyped;
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

/// Marker for string-based paths and expressions whose value type is unknown.
pub enum Untyped {}

/// A reflected location carrying the Rust type of its final value.
/// Obtain one with `path!`; call `erase()` for runtime-checked APIs.
/// Explicit `as Type` shapes are assertions about dynamic data, checked at runtime.
pub struct TypedBindingPath<T: ?Sized> {
    path: BindingPath,
    marker: std::marker::PhantomData<fn(&T) -> &T>,
}

impl<T: ?Sized> TypedBindingPath<T> {
    /// Macro support. The projection is type-checked, never executed.
    /// Like a string-based path constructor, callers constructing this manually
    /// must ensure the supplied path matches the projection.
    #[doc(hidden)]
    pub fn from_projection<Root>(path: BindingPath, _: impl FnOnce(&Root) -> &T) -> Self {
        Self { path, marker: std::marker::PhantomData }
    }

    pub fn erase(self) -> BindingPath { self.path }
}
impl<T: ?Sized> Clone for TypedBindingPath<T> {
    fn clone(&self) -> Self { Self { path: self.path.clone(), marker: std::marker::PhantomData } }
}
impl<T: ?Sized> std::fmt::Debug for TypedBindingPath<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.path.fmt(f) }
}
impl<T: ?Sized> PartialEq for TypedBindingPath<T> {
    fn eq(&self, other: &Self) -> bool { self.path == other.path }
}
impl<T: ?Sized> Eq for TypedBindingPath<T> {}
impl<T: ?Sized> std::hash::Hash for TypedBindingPath<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { std::hash::Hash::hash(&self.path, state); }
}
impl<T: ?Sized> PartialEq<BindingPath> for TypedBindingPath<T> {
    fn eq(&self, other: &BindingPath) -> bool { self.path == *other }
}
impl<T: ?Sized> PartialEq<TypedBindingPath<T>> for BindingPath {
    fn eq(&self, other: &TypedBindingPath<T>) -> bool { *self == other.path }
}
impl<T> IntoBindingPath for TypedBindingPath<T> {
    type Value = T;
    fn into_binding_path(self) -> Result<BindingPath> { Ok(self.erase()) }
}
impl<T> IntoBindingPath for Result<TypedBindingPath<T>> {
    type Value = T;
    fn into_binding_path(self) -> Result<BindingPath> { self.map(TypedBindingPath::erase) }
}
impl<T: ?Sized> From<TypedBindingPath<T>> for BindingPath {
    fn from(path: TypedBindingPath<T>) -> Self { path.erase() }
}

impl BindingPath {
    pub(crate) fn write_value(self, world: &mut World, value: Box<dyn PartialReflect>) -> Result<()> {
        let mut writer = self.writer();
        writer.initialize(world);
        writer.check_change_tick(world.read_change_tick());
        writer.run(Some(value), world)?;
        writer.apply_deferred(world);
        Ok(())
    }
}
