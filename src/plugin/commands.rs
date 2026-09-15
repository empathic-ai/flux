use crate::prelude::*;
use bevy::prelude::*;

/// Deferred, typed writes to reflected source properties.
///
/// ```
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Model { number: i32 }
/// fn update(mut commands: Commands, entity: Entity) {
///     commands.set_property(path!(entity, Model.number), 42);
/// }
/// ```
///
/// Wrong value types are rejected, including macro Results:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// #[derive(Component, Reflect)]
/// struct Model { number: i32 }
/// fn update(mut commands: Commands, entity: Entity) {
///     commands.set_property(path!(entity, Model.number), String::from("wrong"));
/// }
/// ```
/// Computations do not identify a writable source:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// fn update(mut commands: Commands) {
///     commands.set_property(process((), || Ok(42_i32)), 7_i32);
/// }
/// ```
/// String-based paths cannot bypass the type check:
/// ```compile_fail
/// use bevy::prelude::*;
/// use flux::prelude::*;
/// fn update(mut commands: Commands, entity: Entity) {
///     commands.set_property(BindingPath::new(entity, "Model", Some("number")), 7_i32);
/// }
/// ```
pub trait FluxCommandsExt {
    /// Queue a one-shot write; `source` must carry exactly the value's type.
    /// Path resolution and reflection failures use Bevy's command error handler.
    fn set_property<T: PartialReflect>(
        &mut self,
        source: impl IntoBindingPath<Value = T>,
        value: T,
    );
}
impl FluxCommandsExt for Commands<'_, '_> {
    fn set_property<T: PartialReflect>(
        &mut self,
        source: impl IntoBindingPath<Value = T>,
        value: T,
    ) {
        let path = source.into_binding_path();
        self.queue(move |world: &mut World| -> bevy::prelude::Result {
            path?.write_value(world, Box::new(value))?;
            Ok(())
        });
    }
}
