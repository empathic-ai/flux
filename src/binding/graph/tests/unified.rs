use super::*;

fn direct(app: &mut App, source: BindingPath, target: BindingPath) {
    let owner = target.entity;
    let mut graph = BindingGraph::new();
    let node = graph.source(source).unwrap();
    graph
        .bind_with_policy(node, target, BindingWritePolicy::OnSourceChange)
        .unwrap();
    graph.install(app.world_mut(), owner).unwrap();
}

#[test]
fn cached_paths_skip_idle_walks_and_unrelated_field_changes_preserve_edits() {
    let mut app = app();
    let source = model(&mut app, 7, vec![]);
    let a = model(&mut app, 0, vec![]);
    let b = model(&mut app, 0, vec![]);
    direct(&mut app, at(source, "number"), at(a, "number"));
    direct(&mut app, at(source, "number"), at(b, "number"));
    app.update();
    assert_eq!(
        app.world()
            .resource::<BindingGraphs>()
            .runtime
            .as_ref()
            .unwrap()
            .walks,
        3,
        "two graphs share one source walk plus one walk per destination"
    );
    app.update();
    let walks = app
        .world()
        .resource::<BindingGraphs>()
        .runtime
        .as_ref()
        .unwrap()
        .walks;
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(
        app.world()
            .resource::<BindingGraphs>()
            .runtime
            .as_ref()
            .unwrap()
            .walks,
        walks
    );
    app.world_mut().get_mut::<Model>(a).unwrap().number = 99;
    app.world_mut()
        .get_mut::<Model>(source)
        .unwrap()
        .items
        .push(1);
    app.update();
    assert_eq!(app.world().get::<Model>(a).unwrap().number, 99);
    assert_eq!(app.world().get::<Model>(b).unwrap().number, 7);
    app.world_mut().get_mut::<Model>(source).unwrap().number = 8;
    app.update();
    assert_eq!(app.world().get::<Model>(a).unwrap().number, 8);
    assert_eq!(app.world().get::<Model>(b).unwrap().number, 8);
    assert_ok(&app);
}

#[test]
fn graph_sources_can_be_inspected_suspended_and_retargeted_without_reinitializing_systems() {
    let mut app = app();
    let a = model(&mut app, 1, vec![]);
    let b = model(&mut app, 2, vec![]);
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let source = graph.pending_source();
    let computed = graph
        .process_system(source, |In((value,)): In<(i32,)>, mut calls: Local<i32>| {
            *calls += 1;
            Ok(vec![value, *calls])
        })
        .unwrap();
    graph
        .bind_with_policy(
            source,
            at(target, "number"),
            BindingWritePolicy::OnSourceChange,
        )
        .unwrap();
    graph.bind(computed, at(target, "items")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().resource::<BindingGraphs>().iter().count(), 1);
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .iter()
            .next()
            .unwrap()
            .1
            .sources()
            .next()
            .unwrap()
            .1
            .is_none()
    );
    app.world_mut()
        .resource_mut::<BindingGraphs>()
        .set_source(source, Some(at(a, "number")))
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![1, 1]);
    app.world_mut()
        .resource_mut::<BindingGraphs>()
        .set_source(source, Some(at(b, "number")))
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![2, 2]);
    app.world_mut()
        .resource_mut::<BindingGraphs>()
        .set_source(source, None)
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![2, 2]);
    app.world_mut()
        .resource_mut::<BindingGraphs>()
        .set_source(source, Some(at(a, "number")))
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![1, 3]);
    assert!(
        app.world_mut()
            .resource_mut::<BindingGraphs>()
            .set_source(computed, None)
            .is_err()
    );
    assert_ok(&app);
}

#[cfg(feature = "bevy_std")]
#[test]
fn direct_builders_install_graphs_immediately_without_descriptor_entities() {
    use crate::prelude::*;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = app();
    let source = model(&mut app, 7, vec![]);
    let target = model(&mut app, 0, vec![]);
    let entities = app.world().entities().len();
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands
                .entity(target)
                .builder()
                .bind_from(path!(source, Model.number), component_path!(Model.number));
        })
        .unwrap();
    assert_eq!(app.world().entities().len(), entities);
    assert_eq!(app.world().resource::<BindingGraphs>().iter().count(), 1);
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 7);
    assert_eq!(app.world().resource::<BindingGraphs>().iter().count(), 1);
    assert_ok(&app);
}

#[test]
fn reverse_registered_cascade_settles_before_computation_and_computes_once() {
    let mut app = app();
    let nodes: Vec<_> = (0..40).map(|_| model(&mut app, 0, vec![])).collect();
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let source = graph.source(at(*nodes.last().unwrap(), "number")).unwrap();
    let result = graph
        .process_system(source, |In((n,)): In<(i32,)>, mut calls: Local<i32>| {
            *calls += 1;
            Ok(vec![n, *calls])
        })
        .unwrap();
    graph.bind(result, at(target, "items")).unwrap();
    graph.install(app.world_mut(), target).unwrap(); // consumer installed before producers
    for pair in nodes.windows(2).rev() {
        direct(&mut app, at(pair[0], "number"), at(pair[1], "number"));
    }
    app.world_mut().get_mut::<Model>(nodes[0]).unwrap().number = 42;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![42, 1]);
    app.world_mut().get_mut::<Model>(nodes[0]).unwrap().number = 17;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![17, 2]);
    assert_ok(&app);
}

#[test]
fn missing_sources_targets_and_removed_components_recover() {
    let mut app = app();
    let source = app.world_mut().spawn_empty().id();
    let target = app.world_mut().spawn_empty().id();
    direct(&mut app, at(source, "number"), at(target, "number"));
    app.update();
    app.world_mut().entity_mut(source).insert(Model {
        number: 5,
        items: vec![],
        next: None,
    });
    app.update();
    app.world_mut().entity_mut(target).insert(Model {
        number: 0,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 5);
    app.world_mut().entity_mut(target).remove::<Model>();
    app.update();
    app.world_mut().entity_mut(target).insert(Model {
        number: 0,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 5);
    app.world_mut().entity_mut(source).remove::<Model>();
    app.update();
    app.world_mut().get_mut::<Model>(target).unwrap().number = 99;
    app.world_mut().entity_mut(source).insert(Model {
        number: 5,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 5);
    assert_ok(&app);
}

#[test]
fn record_mapping_changes_retarget_equal_values_and_remove_stale_dependencies() {
    let mut app = app();
    let source = model(&mut app, 0, vec![]);
    let a = model(&mut app, 7, vec![]);
    let b = model(&mut app, 7, vec![]);
    let target = model(&mut app, 0, vec![]);
    let id = Id::new();
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, a);
    app.world_mut().get_mut::<Model>(source).unwrap().next = Some(id.clone());
    direct(
        &mut app,
        at(source, "next.Model.number"),
        at(target, "number"),
    );
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 7);
    app.world_mut().get_mut::<Model>(target).unwrap().number = 99;
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, b);
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 7);
    app.world_mut().get_mut::<Model>(target).unwrap().number = 99;
    app.world_mut().get_mut::<Model>(a).unwrap().number = 21;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 99);
    app.world_mut().get_mut::<Model>(b).unwrap().number = 22;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 22);
    assert_ok(&app);
}

#[test]
fn source_nodes_deduplicate_and_owner_cleanup_releases_cached_values() {
    let mut app = app();
    let source = model(&mut app, 7, vec![]);
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let a = graph.source(at(source, "number")).unwrap();
    let b = graph.source(at(source, "number")).unwrap();
    assert_eq!(a, b);
    graph.bind(a, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    app.world_mut().despawn(target);
    app.update();
    let graphs = app.world().resource::<BindingGraphs>();
    assert!(graphs.graphs.is_empty());
    assert!(
        graphs
            .runtime
            .as_ref()
            .unwrap()
            .dependency_keys()
            .is_empty()
    );
}

#[test]
fn nonconverging_opaque_cycle_is_bounded_and_reported() {
    #[derive(Component, Reflect, Clone)]
    #[reflect(opaque)]
    #[reflect(Clone)]
    struct Opaque(i32);
    impl Reactive for Opaque {}
    let mut app = app();
    app.register_component_as::<dyn Reactive, Opaque>();
    let a = app.world_mut().spawn(Opaque(1)).id();
    let b = app.world_mut().spawn(Opaque(2)).id();
    let at = |entity| BindingPath::new(entity, "Opaque", None).unwrap();
    direct(&mut app, at(a), at(b));
    direct(&mut app, at(b), at(a));
    let independent = model(&mut app, 7, vec![]);
    let output = model(&mut app, 0, vec![]);
    let computed = model(&mut app, 0, vec![]);
    direct(
        &mut app,
        super::at(independent, "number"),
        super::at(output, "number"),
    );
    let mut graph = BindingGraph::new();
    let node = graph.constant(8_i32).unwrap();
    graph.bind(node, super::at(computed, "number")).unwrap();
    graph.install(app.world_mut(), computed).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(output).unwrap().number, 7);
    assert_eq!(app.world().get::<Model>(computed).unwrap().number, 8);
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .any(|(_, error)| error.contains("work budget"))
    );
    // A cyclic component with no reflected equality cannot prove convergence.
    app.world_mut().despawn(a);
    app.world_mut().despawn(b);
    app.update();
    assert_ok(&app);
}

#[test]
fn missing_database_resource_is_a_diagnostic_and_cache_eviction_does_not_lose_updates() {
    let mut app = app();
    let source = model(&mut app, 7, vec![]);
    let target = model(&mut app, 0, vec![]);
    direct(&mut app, at(source, "number"), at(target, "number"));
    app.update();
    let db = app.world_mut().remove_resource::<DBConfig>().unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .any(|(_, error)| error.contains("DBConfig"))
    );
    app.world_mut().get_mut::<Model>(source).unwrap().number = 8;
    app.world_mut().insert_resource(db);
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 8);
    assert_ok(&app);
}
