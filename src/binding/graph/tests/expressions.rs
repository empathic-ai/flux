use super::*;
use crate::prelude::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn nested_expressions_accept_paths_results_and_named_functions() {
    fn sum(a: i32, b: i32, c: i32, d: i32) -> Result<i32> {
        Ok(a + b + c + d)
    }
    let mut app = app();
    let source = model(&mut app, 3, vec![]);
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let expr = process(
        (
            at(source, "number"),
            binding_path!(source, Model.number),
            process((), || Ok(5_i32)),
            Ok(process((at(source, "number"),), |n: i32| Ok(n * 2))),
        ),
        sum,
    );
    let node = graph.add(process(expr, |n: i32| Ok(n * 2))).unwrap();
    graph.bind(node, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 34);
    app.world_mut().get_mut::<Model>(source).unwrap().number = 4;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 42);
    assert_ok(&app);
}

#[test]
fn failed_expression_add_rolls_back_nodes_and_keeps_existing_handles_valid() {
    let mut graph = BindingGraph::new();
    let original = graph.constant(7_i32).unwrap();
    let expr = process(
        (
            process((), || Ok(1_i32)),
            Err::<BindingPath, _>(anyhow!("bad source")),
        ),
        |a: i32, b: i32| Ok(a + b),
    );
    assert!(
        graph
            .add(expr)
            .unwrap_err()
            .to_string()
            .contains("bad source")
    );
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.process(original, |n: i32| Ok(n)).is_ok());
    assert!(
        graph
            .add(Err::<BindingExpr, _>(anyhow!("bad expression")))
            .is_err()
    );
    assert_eq!(graph.nodes.len(), 2);
}

#[test]
fn expression_systems_preserve_local_state_and_refresh_resources() {
    let mut app = app();
    app.insert_resource(Threshold(10));
    let source = app.world_mut().spawn_empty().id();
    let target = model(&mut app, 99, vec![]);
    let expr = process_system(
        binding_path!(source, Model.number),
        |In((n,)): In<(i32,)>, threshold: Res<Threshold>, mut calls: Local<i32>| {
            *calls += 1;
            Ok(n + threshold.0 + *calls)
        },
    );
    let mut expr = Some(expr);
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands
                .entity(target)
                .builder()
                .bind_from(expr.take().unwrap(), component_path!(Model.number));
        })
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 99);
    app.world_mut().entity_mut(source).insert(Model {
        number: 2,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 13);
    app.world_mut().resource_mut::<Threshold>().0 = 20;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 24);
    assert_ok(&app);
    app.world_mut().despawn(target);
    app.update();
    assert!(app.world().resource::<BindingGraphs>().graphs.is_empty());
}

#[test]
fn fallible_builders_do_not_queue_partial_installations() {
    let mut app = app();
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            let error = commands
                .entity(owner)
                .builder()
                .try_bind_list_from(
                    process(Err::<BindingPath, _>(anyhow!("bad list")), |n: i32| {
                        Ok(vec![n])
                    }),
                    |_: In<Entity>| {},
                )
                .err()
                .unwrap();
            assert!(error.to_string().contains("bad list"));
            assert!(
                commands
                    .entity(owner)
                    .builder()
                    .try_bind_from(
                        process((), || Ok(1_i32)),
                        Err::<ComponentBindingPath, _>(anyhow!("bad target")),
                    )
                    .is_err()
            );
        })
        .unwrap();
    assert!(app.world().get::<ReactiveListView>(owner).is_none());
    assert!(app.world().resource::<BindingGraphs>().graphs.is_empty());
}

#[test]
fn expressions_support_sixteen_inputs_and_zero_input_systems() {
    let mut app = app();
    let target = model(&mut app, 0, vec![]);
    let value = || process((), || Ok(1_i32));
    let expr = process(
        (
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
            value(),
        ),
        |a: i32,
         b: i32,
         c: i32,
         d: i32,
         e: i32,
         f: i32,
         g: i32,
         h: i32,
         i: i32,
         j: i32,
         k: i32,
         l: i32,
         m: i32,
         n: i32,
         o: i32,
         p: i32| Ok(a + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p),
    );
    let mut graph = BindingGraph::new();
    let node = graph
        .add(process(
            (expr, process_system((), |In(()): In<()>| Ok(2_i32))),
            |a: i32, b: i32| Ok(a + b),
        ))
        .unwrap();
    graph.bind(node, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 18);
    assert_ok(&app);
}
