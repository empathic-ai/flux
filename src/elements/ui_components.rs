// DOM rendering needs ECS state, but not Bevy's GPU UI/text/render pipeline.
// Keep the Bevy types available for consumers that select that renderer.
#[cfg(feature = "bevy_ui")]
pub use bevy::prelude::{BackgroundColor, Button};

#[cfg(not(feature = "bevy_ui"))]
mod dom {
    use bevy::prelude::*;
    use serde::{Deserialize, Serialize};

    /// Fill color consumed by the DOM renderer, transparent by default.
    #[derive(Component, Copy, Clone, Debug, PartialEq, Reflect, Serialize, Deserialize)]
    #[reflect(Component, Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
    pub struct BackgroundColor(pub Color);

    impl BackgroundColor {
        pub const DEFAULT: Self = Self(Color::NONE);
    }

    impl Default for BackgroundColor {
        fn default() -> Self {
            Self::DEFAULT
        }
    }

    impl<T: Into<Color>> From<T> for BackgroundColor {
        fn from(color: T) -> Self {
            Self(color.into())
        }
    }

    /// Button marker; pointer state and handlers are owned by InteractState.
    #[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Reflect)]
    #[reflect(Component, Default, Debug, PartialEq, Clone)]
    #[require(Transform, BackgroundColor)]
    pub struct Button;
}

#[cfg(not(feature = "bevy_ui"))]
pub use dom::{BackgroundColor, Button};

#[cfg(all(test, not(feature = "bevy_ui")))]
mod tests {
    use super::*;
    use crate::prelude::{Control, ImageRect};
    use bevy::prelude::*;

    #[test]
    fn dom_controls_keep_render_defaults_without_gpu_ui() {
        let mut world = World::new();
        let control = world.spawn(Control::default()).id();
        let image = world.spawn(ImageRect::default()).id();
        let button = world.spawn(Button).id();
        for entity in [control, image, button] {
            assert_eq!(world.get::<Transform>(entity).unwrap().scale, Vec3::ONE);
            assert_eq!(world.get::<BackgroundColor>(entity).unwrap().0, Color::NONE);
        }
    }

    #[test]
    fn explicit_style_and_hover_transform_survive_required_components() {
        let mut world = World::new();
        let entity = world
            .spawn((
                Control::default(),
                Button,
                BackgroundColor(Color::WHITE),
                Transform::from_scale(Vec3::splat(1.005)),
            ))
            .id();
        world.entity_mut(entity).insert(ImageRect::default());
        assert_eq!(
            world.get::<BackgroundColor>(entity).unwrap().0,
            Color::WHITE
        );
        assert_eq!(
            world.get::<Transform>(entity).unwrap().scale,
            Vec3::splat(1.005)
        );
    }
}
