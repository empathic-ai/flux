use super::*;
use crate::prelude::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn macro_preserves_leaf_types_without_evaluating_projections() {
    #[derive(Component, Reflect)]
    struct Nested {
        inner: Option<Model>,
        pair: (String, i32),
    }
    fn typed<T>(_: Result<TypedBindingPath<T>>) {}
    let entity = Entity::PLACEHOLDER;
    typed::<Model>(path!(entity, Model));
    typed::<i32>(path!(entity, Model.number));
    typed::<Option<Id>>(path!(entity, Model.next));
    typed::<i32>(path!(entity, Nested.inner?.items[999]));
    typed::<String>(path!(entity, Nested.pair.0));
    typed::<Vec<i32>>(path!(entity, Model.next -> Model.items));
    typed::<i32>(path!(entity, ReactiveView.value as Model.number));
    typed::<i32>(path!(entity, ReactiveView.value as Id -> Model.number));
    let _: BindingExpr<i32> = path!(entity, Model.number).into_binding_expr();
    let _: BindingExpr<String> = process(path!(entity, Model.number), |n: i32| {
        Ok(n.to_string())
    });
    let _: BindingExpr<String> = process_system(
        path!(entity, Model.number),
        |In((n,)): In<(i32,)>| Ok(n.to_string()),
    );
}

#[test]
fn commands_write_once_and_follow_the_route_at_application_time() {
    let mut app = app();
    let source = model(&mut app, 1, vec![2]);
    let target = model(&mut app, 3, vec![4]);
    let id = Id::new();
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, target);
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands.set_property(path!(source, Model.number), 7);
            commands.set_property(path!(source, Model.items[0]).unwrap(), 8);
            commands.set_property(path!(source, Model.next), Some(id));
            // The preceding command sets the reference this write will follow.
            commands.set_property(path!(source, Model.next -> Model.number), 9);
        })
        .unwrap();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 7);
    assert_eq!(app.world().get::<Model>(source).unwrap().items, vec![8]);
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 9);
    app.world_mut().get_mut::<Model>(source).unwrap().number = 12;
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 12);
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .iter()
            .next()
            .is_none()
    );
}

#[test]
fn command_writes_through_asserted_dynamic_shape() {
    let mut app = app();
    app.register_component_as::<dyn Reactive, ReactiveView>();
    let source = app
        .world_mut()
        .spawn(ReactiveView {
            value: Dynamic::new(&Model {
                number: 1,
                items: vec![],
                next: None,
            }),
        })
        .id();
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands.set_property(path!(source, ReactiveView.value as Model.number), 5);
        })
        .unwrap();
    let view = app.world().get::<ReactiveView>(source).unwrap();
    assert_eq!(Model::from_reflect(view.value.as_ref()).unwrap().number, 5);
}

#[test]
fn explicit_erasure_preserves_dynamic_expression_support() {
    let source = Entity::PLACEHOLDER;
    let path: BindingPath = path!(source, Model.number).unwrap().erase();
    let _: BindingExpr<String> = process(path, |s: String| Ok(s));
    let erased: BindingExpr = process((), || Ok(1_i32)).erase();
    let _: BindingExpr<String> = process(erased, |s: String| Ok(s));
}

#[test]
fn typed_paths_keep_runtime_resolution_failures_fallible() {
    let mut app = app();
    let source = model(&mut app, 1, vec![]);
    let missing_index = path!(source, Model.items[0]).unwrap().erase();
    assert!(
        missing_index
            .write_value(app.world_mut(), Box::new(2_i32))
            .is_err()
    );
    let missing_reference = path!(source, Model.next -> Model.number)
        .unwrap()
        .erase();
    assert!(
        missing_reference
            .write_value(app.world_mut(), Box::new(3_i32))
            .is_err()
    );
    let missing_component = path!(Entity::PLACEHOLDER, Model.number)
        .unwrap()
        .erase();
    assert!(
        missing_component
            .write_value(app.world_mut(), Box::new(4_i32))
            .is_err()
    );
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 1);
}

#[test]
fn closure_parameter_types_are_inferred_from_paths_and_nested_expressions() {
    use std::collections::HashMap;
    #[derive(Component, Reflect)]
    struct Networks {
        available: HashMap<i32, String>,
        configured: HashMap<i32, String>,
    }
    impl Reactive for Networks {}
    let mut app = app();
    app.register_component_as::<dyn Reactive, Networks>();
    let source = app
        .world_mut()
        .spawn(Networks {
            available: [3, 1, 2]
                .into_iter()
                .map(|id| (id, id.to_string()))
                .collect(),
            configured: [(2, "configured".to_owned())].into_iter().collect(),
        })
        .id();
    let target = model(&mut app, 0, vec![]);
    let ids = process(
        (
            path!(source, Networks.available),
            path!(source, Networks.configured),
        ),
        |available, configured| {
            let mut ids: Vec<_> = available
                .into_keys()
                .filter(|id| !configured.contains_key(id))
                .collect();
            ids.sort_unstable();
            Ok(ids)
        },
    );
    let length = process(ids, |ids| Ok(ids.len() as i32));
    let mut graph = BindingGraph::new();
    let node = graph.add(length).unwrap();
    graph
        .bind(node, path!(target, Model.number))
        .unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 2);
    assert_ok(&app);
}
