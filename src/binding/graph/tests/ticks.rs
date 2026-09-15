use super::*;
use bevy::ecs::component::Tick;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct TickWarnings(Arc<AtomicUsize>);
impl tracing::Subscriber for TickWarnings {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Target(bool);
        impl tracing::field::Visit for Target {
            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "log.target" && value == "bevy_ecs::system::system" {
                    self.0 = true;
                }
            }
        }
        let mut target = Target(event.metadata().target() == "bevy_ecs::system::system");
        event.record(&mut target);
        if target.0 && *event.metadata().level() == tracing::Level::WARN {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn tick_warnings(run: impl FnOnce()) -> usize {
    // Bevy ECS emits these diagnostics through log, not tracing directly.
    // Install its bridge once; per-thread subscribers isolate each test's count.
    static LOG_BRIDGE: std::sync::Once = std::sync::Once::new();
    LOG_BRIDGE.call_once(|| {
        use bevy::log::tracing_subscriber::util::SubscriberInitExt;
        let _ = bevy::log::tracing_subscriber::registry().try_init();
    });
    let warnings = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(TickWarnings(warnings.clone()), run);
    warnings.load(Ordering::Relaxed)
}

#[test]
fn delayed_validation_preserves_first_run_changed_flags_without_age_warnings() {
    assert_eq!(
        tick_warnings(|| {
            let mut app = app();
            let source = model(&mut app, 1, vec![]);
            let target = model(&mut app, 0, vec![]);
            let mut graph = BindingGraph::new();
            let output = graph
                .system(
                    "changed flags",
                    &[],
                    move |_: In<BindingInputs>,
                          resource: Res<Threshold>,
                          query: Query<Ref<Model>>| {
                        Ok(Some(Box::new(vec![
                            i32::from(resource.is_changed()),
                            i32::from(query.get(source).unwrap().is_changed()),
                        ]) as Box<dyn PartialReflect>))
                    },
                )
                .unwrap();
            graph.bind(output, at(target, "items")).unwrap();
            graph.install(app.world_mut(), target).unwrap();
            for _ in 0..3 {
                app.update();
            } // validation fails; nothing has run
            app.insert_resource(Threshold(1));
            app.update();
            assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![1, 1]);
            app.update();
            assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![0, 0]);
            app.world_mut().get_mut::<Model>(source).unwrap().number += 1;
            app.world_mut().resource_mut::<Threshold>().0 += 1;
            app.update();
            assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![1, 1]);
            assert_ok(&app);
        }),
        0
    );
}

#[test]
fn typed_adapter_skips_are_not_mistaken_for_the_inner_systems_first_run() {
    assert_eq!(
        tick_warnings(|| {
            let mut app = app();
            app.insert_resource(Threshold(1));
            let source = app.world_mut().spawn_empty().id();
            let target = model(&mut app, 0, vec![]);
            let mut graph = BindingGraph::new();
            let input = graph.source(at(source, "number")).unwrap();
            let output = graph
                .process_system(
                    input,
                    |In((number,)): In<(i32,)>, resource: Res<Threshold>, mut calls: Local<i32>| {
                        *calls += 1;
                        Ok(vec![number, i32::from(resource.is_changed()), *calls])
                    },
                )
                .unwrap();
            graph.bind(output, at(target, "items")).unwrap();
            graph.install(app.world_mut(), target).unwrap();
            for _ in 0..4 {
                app.update();
            }
            app.world_mut().entity_mut(source).insert(Model {
                number: 7,
                items: vec![],
                next: None,
            });
            app.update();
            assert_eq!(
                app.world().get::<Model>(target).unwrap().items,
                vec![7, 1, 1]
            );
            app.update();
            assert_eq!(
                app.world().get::<Model>(target).unwrap().items,
                vec![7, 0, 2]
            );
            assert_ok(&app);
        }),
        0
    );
}

#[test]
fn late_sink_and_two_way_reader_route_and_writer_starts_do_not_warn() {
    assert_eq!(
        tick_warnings(|| {
            let mut app = app();
            let source = model(&mut app, 7, vec![]);
            let target = model(&mut app, 7, vec![]);
            let sink = model(&mut app, 7, vec![]);
            let mut graph = BindingGraph::new();
            let value = graph.constant(7_i32).unwrap();
            graph.bind(value, at(sink, "number")).unwrap();
            graph.two_way(
                at(source, "number"),
                at(target, "number"),
                BindingConflict::Reject,
            );
            graph.install(app.world_mut(), sink).unwrap();
            for _ in 0..4 {
                app.update();
            } // all writers remain unused
            app.world_mut().get_mut::<Model>(sink).unwrap().number = 0;
            app.world_mut().get_mut::<Model>(source).unwrap().number = 8;
            app.update();
            assert_eq!(app.world().get::<Model>(sink).unwrap().number, 7);
            assert_eq!(app.world().get::<Model>(target).unwrap().number, 8);
            app.world_mut().get_mut::<Model>(target).unwrap().number = 9;
            app.update();
            assert_eq!(app.world().get::<Model>(source).unwrap().number, 9);
            assert_ok(&app);
        }),
        0
    );
}

#[test]
fn genuinely_old_executed_systems_still_get_bevys_age_check() {
    assert_eq!(
        tick_warnings(|| {
            let mut world = World::new();
            let mut system = GraphSystem::new(Box::new(IntoSystem::into_system(|| {})));
            system.initialize(&mut world);
            world.increment_change_tick();
            system.check_change_tick(world.read_change_tick());
            assert_eq!(
                system.get_last_run(),
                Tick::new(world.read_change_tick().get().wrapping_sub(Tick::MAX.get()))
            );
            system.run((), &mut world);
            let last = system.get_last_run();
            system.check_change_tick(world.read_change_tick());
            assert_eq!(
                system.get_last_run(),
                last,
                "ordinary checks must not erase change history"
            );
            let stale_now = Tick::new(last.get().wrapping_add(Tick::MAX.get()).wrapping_add(1));
            system.check_change_tick(stale_now);
            assert_eq!(
                system.get_last_run(),
                Tick::new(stale_now.get().wrapping_sub(Tick::MAX.get()))
            );
        }),
        1
    );
}
