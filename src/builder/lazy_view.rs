//! One-shot child construction driven by effective Control visibility.
use crate::prelude::{BindingGraphSet, Control, Route};
use bevy::{ecs::world::CommandQueue, prelude::*};

/// Present only while a view's child factory is waiting to run.
/// Despawning an unvisited view also drops its captured inputs.
#[derive(Component, Default)]
pub struct LazyView(Vec<Box<dyn FnOnce(&mut World, Entity) + Send + Sync>>);

impl LazyView {
    pub fn new<F>(build: F) -> Self
    where
        F: FnOnce(Entity, &mut ChildSpawnerCommands<'_>) + Send + Sync + 'static,
    {
        Self(vec![Box::new(move |world, entity| {
            let mut queue = CommandQueue::default();
            Commands::new(&mut queue, world)
                .entity(entity)
                .with_children(|parent| build(entity, parent));
            queue.apply(world);
        })])
    }

    pub(crate) fn append(&mut self, mut other: Self) {
        self.0.append(&mut other.0);
    }
}

/// Routing adapters insert this after resolving a Route's Control visibility.
/// An unresolved route blocks lazy construction, including in its descendants.
#[derive(Component)]
pub struct RouteVisibilityResolved;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LazyViewSet;

/// Included by FluxPlugin; usable alone in UI-only applications and tests.
pub struct LazyViewPlugin;
impl Plugin for LazyViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            mount_visible_views
                .in_set(LazyViewSet)
                .after(BindingGraphSet),
        );
    }
}

fn effectively_visible(world: &World, mut entity: Entity) -> bool {
    let root = entity;
    loop {
        let Ok(current) = world.get_entity(entity) else {
            return false;
        };
        if (entity != root && current.contains::<LazyView>())
            || current
                .get::<Control>()
                .is_some_and(|control| !control.is_visible)
            || (current.contains::<Route>() && !current.contains::<RouteVisibilityResolved>())
        {
            return false;
        }
        match current.get::<ChildOf>() {
            Some(parent) => entity = parent.parent(),
            None => return true,
        }
    }
}

#[derive(Resource)]
struct PendingViews(QueryState<Entity, With<LazyView>>);

/// Mount visible pending views, including newly created visible lazy descendants.
/// Only pending factories are scanned; mounted views leave the query permanently.
/// Routing may queue this before ShowView so observers see the constructed tree.
pub fn mount_visible_views(world: &mut World) {
    if !world.contains_resource::<PendingViews>() {
        let query = world.query_filtered::<Entity, With<LazyView>>();
        world.insert_resource(PendingViews(query));
    }
    loop {
        // Cache archetype matching across frames, including after all views mount.
        let pending: Vec<Entity> = world
            .resource_scope(|world, mut query: Mut<PendingViews>| query.0.iter(world).collect());
        let mut mounted = false;
        for entity in pending {
            // Check at execution time: an earlier factory can change the hierarchy.
            if !effectively_visible(world, entity) {
                continue;
            }
            let Some(factory) = world.entity_mut(entity).take::<LazyView>() else {
                continue;
            };
            for build in factory.0 {
                if world.get_entity(entity).is_err() {
                    break;
                }
                build(world, entity);
            }
            mounted = true;
        }
        if !mounted {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Component)]
    struct Content;

    fn pending(world: &mut World, visible: bool) -> Entity {
        world
            .spawn((
                Control {
                    is_visible: visible,
                    ..default()
                },
                LazyView::new(|_, children| {
                    children.spawn(Content);
                }),
            ))
            .id()
    }

    #[test]
    fn hidden_view_builds_on_first_show_and_retains_its_children() {
        let mut world = World::new();
        let root = pending(&mut world, false);
        mount_visible_views(&mut world);
        assert!(world.get::<Children>(root).is_none());
        world.get_mut::<Control>(root).unwrap().is_visible = true;
        mount_visible_views(&mut world);
        let child = world.get::<Children>(root).unwrap()[0];
        assert!(world.get::<Content>(child).is_some());
        assert!(world.get::<LazyView>(root).is_none());
        for visible in [false, true, false, true] {
            world.get_mut::<Control>(root).unwrap().is_visible = visible;
            mount_visible_views(&mut world);
            assert_eq!(&world.get::<Children>(root).unwrap()[..], &[child]);
        }
    }

    #[test]
    fn hidden_ancestors_and_reparenting_control_mounting() {
        let mut world = World::new();
        let hidden = world
            .spawn(Control {
                is_visible: false,
                ..default()
            })
            .id();
        let transparent = world.spawn(ChildOf(hidden)).id();
        let root = pending(&mut world, true);
        world.entity_mut(root).insert(ChildOf(transparent));
        mount_visible_views(&mut world);
        assert!(world.get::<Children>(root).is_none());
        let visible = world.spawn_empty().id();
        world.entity_mut(transparent).insert(ChildOf(visible));
        mount_visible_views(&mut world);
        assert!(world.get::<Children>(root).is_some());
    }

    #[test]
    fn unresolved_and_unselected_routes_never_mount_descendants() {
        let mut world = World::new();
        let route = world
            .spawn((
                Control::default(),
                Route {
                    name: "devices".into(),
                },
            ))
            .id();
        let root = pending(&mut world, true);
        world.entity_mut(root).insert(ChildOf(route));
        mount_visible_views(&mut world);
        assert!(world.get::<Children>(root).is_none());
        world.entity_mut(route).insert(RouteVisibilityResolved);
        world.get_mut::<Control>(route).unwrap().is_visible = false;
        mount_visible_views(&mut world);
        assert!(world.get::<Children>(root).is_none());
        world.get_mut::<Control>(route).unwrap().is_visible = true;
        mount_visible_views(&mut world);
        assert!(world.get::<Children>(root).is_some());
    }

    #[test]
    fn nested_factories_mount_in_one_pass_but_hidden_siblings_wait() {
        let mut world = World::new();
        let root = world
            .spawn((
                Control::default(),
                LazyView::new(|_, children| {
                    children.child().lazy_children(|children| {
                        children.spawn(Content);
                    });
                    children
                        .child()
                        .is_visible(false)
                        .lazy_children(|children| {
                            children.spawn(Content);
                        });
                }),
            ))
            .id();
        mount_visible_views(&mut world);
        let children = world.get::<Children>(root).unwrap();
        assert!(world.get::<Children>(children[0]).is_some());
        assert!(world.get::<Children>(children[1]).is_none());
        assert!(world.get::<LazyView>(children[1]).is_some());
    }

    #[test]
    fn repeated_builder_calls_append_children_in_order() {
        let mut world = World::new();
        let mut queue = CommandQueue::default();
        let root = Commands::new(&mut queue, &world)
            .child()
            .lazy_children(|children| {
                children.spawn(Name::new("first"));
            })
            .lazy_children(|children| {
                children.spawn(Name::new("second"));
            })
            .id();
        queue.apply(&mut world);
        mount_visible_views(&mut world);
        let children = world.get::<Children>(root).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(world.get::<Name>(children[0]).unwrap().as_str(), "first");
        assert_eq!(world.get::<Name>(children[1]).unwrap().as_str(), "second");
    }

    #[test]
    fn route_observers_see_children_on_first_event() {
        let mut world = World::new();
        let root = pending(&mut world, false);
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = calls.clone();
        world.entity_mut(root).observe(
            move |event: Trigger<ShowView>,
                  children: Query<&Children>,
                  content: Query<&Content>| {
                let child = children.get(event.target()).unwrap()[0];
                assert!(content.get(child).is_ok());
                observed_calls.fetch_add(1, Ordering::SeqCst);
            },
        );
        world.flush();
        world.get_mut::<Control>(root).unwrap().is_visible = true;
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        commands.queue(mount_visible_views);
        commands.trigger_targets(ShowView { params: default() }, root);
        queue.apply(&mut world);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn despawn_drops_unused_factory_without_building() {
        struct Capture(Arc<AtomicUsize>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let capture = Capture(drops.clone());
        let mut world = World::new();
        let root = world
            .spawn((
                Control {
                    is_visible: false,
                    ..default()
                },
                LazyView::new(move |_, _| {
                    drop(capture);
                    panic!("hidden factory must not run");
                }),
            ))
            .id();
        world.despawn(root);
        mount_visible_views(&mut world);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn visibility_changes_in_binding_phase_mount_in_same_update() {
        let mut app = App::new();
        app.add_plugins(LazyViewPlugin);
        let root = pending(app.world_mut(), false);
        app.add_systems(
            PostUpdate,
            (move |mut controls: Query<&mut Control>| {
                controls.get_mut(root).unwrap().is_visible = true;
            })
            .in_set(BindingGraphSet),
        );
        app.update();
        assert!(app.world().get::<Children>(root).is_some());
    }
}
