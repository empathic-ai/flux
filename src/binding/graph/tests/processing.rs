use super::*;
use crate::prelude::path;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn typed_functions_support_named_functions_and_tuple_arities() {
    fn join(a: i32, b: String, c: Vec<i32>, d: Option<i32>) -> Result<Vec<i32>> {
        Ok(vec![
            a,
            b.len() as i32,
            c.iter().sum(),
            d.unwrap_or_default(),
        ])
    }
    let mut app = app();
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let a = graph.process((), || Ok(2_i32)).unwrap();
    let b = graph.process(a, |a: i32| Ok(a.to_string())).unwrap();
    let c = graph.process((a,), |a: i32| Ok(vec![a, a])).unwrap();
    let d = graph.constant(None::<i32>).unwrap();
    let output = graph.process((a, b, c, d), join).unwrap();
    graph.bind(output, at(target, "items")).unwrap();
    let sum = graph
        .process(
            (a, a, a, a, a, a, a, a, a, a, a, a, a, a, a, a),
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
             p: i32| {
                Ok(a + b + c + d + e + f + g + h + i + j + k + l + m + n + o + p)
            },
        )
        .unwrap();
    graph.bind(sum, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(
        app.world().get::<Model>(target).unwrap().items,
        vec![2, 1, 4, 0]
    );
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 32);
    assert_ok(&app);
}

#[test]
fn missing_inputs_skip_functions_and_type_errors_recover_without_writes() {
    let mut app = app();
    let source = app.world_mut().spawn_empty().id();
    let target = model(&mut app, 99, vec![]);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut graph = BindingGraph::new();
    let source_node = graph.source(at(source, "number")).unwrap();
    let good = graph.constant(1_i32).unwrap();
    let wrong = graph.constant("wrong".to_string()).unwrap();
    let value = graph
        .process((source_node, good), move |a: i32, b: i32| {
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(a + b)
        })
        .unwrap();
    graph.bind(value, at(target, "number")).unwrap();
    graph
        .process((source_node, wrong), |_: i32, _: i32| Ok(0_i32))
        .unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_ok(&app); // missing dominates even if another input has the wrong type
    app.world_mut().entity_mut(source).insert(Model {
        number: 4,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let errors: Vec<_> = app.world().resource::<BindingGraphs>().errors().collect();
    assert!(errors[0].1.contains("input 2 expected i32"));
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 99);
    app.world_mut().entity_mut(source).remove::<Model>();
    app.update();
    assert_ok(&app);
}

#[test]
fn callback_errors_and_input_type_changes_recover() {
    use crate::prelude::{Dynamic, ReactiveView};
    let mut app = app();
    app.register_component_as::<dyn Reactive, ReactiveView>();
    let source = app
        .world_mut()
        .spawn(ReactiveView {
            value: Dynamic::new(&"wrong".to_string()),
        })
        .id();
    let target = model(&mut app, 99, vec![]);
    let mut graph = BindingGraph::new();
    let input = graph
        .source(path!(source, ReactiveView.value).unwrap())
        .unwrap();
    let result = graph
        .process(input, |n: i32| {
            ensure!(n >= 0, "negative value");
            Ok(n * 2)
        })
        .unwrap();
    graph.bind(result, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .next()
            .unwrap()
            .1
            .contains("input 1 expected i32")
    );
    app.world_mut()
        .get_mut::<ReactiveView>(source)
        .unwrap()
        .value = Dynamic::new(&-1_i32);
    app.update();
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .next()
            .unwrap()
            .1
            .contains("negative value")
    );
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 99);
    app.world_mut()
        .get_mut::<ReactiveView>(source)
        .unwrap()
        .value = Dynamic::new(&3_i32);
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 6);
    assert_ok(&app);
}

#[test]
fn typed_systems_preserve_state_skip_missing_and_refresh_ecs_dependencies() {
    fn compute(
        In((a, b)): In<(i32, String)>,
        threshold: Res<Threshold>,
        scores: Query<&Score>,
        mut calls: Local<i32>,
    ) -> Result<i32> {
        *calls += 1;
        Ok(a + b.len() as i32
            + threshold.0
            + scores.iter().map(|score| score.0).sum::<i32>()
            + *calls)
    }
    let mut app = app();
    app.insert_resource(Threshold(10));
    let score = app.world_mut().spawn(Score(20)).id();
    let source = app.world_mut().spawn_empty().id();
    let target = model(&mut app, 99, vec![]);
    let mut graph = BindingGraph::new();
    let a = graph.source(at(source, "number")).unwrap();
    let b = graph.constant("hi".to_string()).unwrap();
    let result = graph.process_system((a, b), compute).unwrap();
    graph.bind(result, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 99);
    app.world_mut().entity_mut(source).insert(Model {
        number: 3,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 36);
    app.world_mut().entity_mut(score).remove::<Score>();
    app.world_mut().resource_mut::<Threshold>().0 = 5;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 12);
    app.world_mut().entity_mut(source).remove::<Model>();
    app.update();
    app.world_mut().entity_mut(source).insert(Model {
        number: 3,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 13);
    assert_ok(&app);
}

#[test]
fn systems_support_zero_single_inputs_and_reject_commands_and_foreign_nodes() {
    let mut app = app();
    app.insert_resource(Threshold(7));
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let a = graph
        .process_system((), |_: In<()>, threshold: Res<Threshold>| Ok(threshold.0))
        .unwrap();
    let b = graph
        .process_system(a, |In((a,)): In<(i32,)>| Ok(a * 2))
        .unwrap();
    graph.bind(b, at(target, "number")).unwrap();
    let mut other = BindingGraph::new();
    assert!(
        other
            .process_system(a, |In((a,)): In<(i32,)>| Ok(a))
            .is_err()
    );
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 14);
    app.world_mut().remove_resource::<Threshold>();
    app.update();
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .next()
            .is_some()
    );
    app.insert_resource(Threshold(8));
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 16);
    assert_ok(&app);
    let mut invalid = BindingGraph::new();
    invalid
        .process_system((), |_: In<()>, mut commands: Commands| {
            commands.spawn_empty();
            Ok(0_i32)
        })
        .unwrap();
    assert!(
        invalid
            .install(app.world_mut(), target)
            .unwrap_err()
            .to_string()
            .contains("deferred writes")
    );
}
