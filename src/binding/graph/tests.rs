use super::*;
use bevy_trait_query::RegisterExt;
mod processing;
#[cfg(feature = "bevy_std")]
mod maps;
mod unified;
#[cfg(feature = "bevy_std")]
mod ticks;
#[cfg(feature = "bevy_std")]
mod expressions;

#[derive(Component, Reflect, Clone, PartialEq)]
#[reflect(PartialEq)]
struct Model {
    number: i32,
    items: Vec<i32>,
    next: Option<Id>,
}
impl Reactive for Model {}

#[derive(Resource)]
struct Threshold(i32);
#[derive(Component)]
struct Score(i32);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(BindingGraphPlugin);
    app.register_component_as::<dyn Reactive, Model>();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    });
    app
}
fn model(app: &mut App, number: i32, items: Vec<i32>) -> Entity {
    app.world_mut()
        .spawn(Model {
            number,
            items,
            next: None,
        })
        .id()
}
fn at(entity: Entity, path: &str) -> BindingPath {
    BindingPath::new(entity, "Model", Some(path)).unwrap()
}
fn number(value: &BindingValue) -> i32 {
    i32::from_reflect(value.as_ref().unwrap().as_ref()).unwrap()
}
fn assert_ok(app: &App) {
    assert_eq!(app.world().resource::<BindingGraphs>().errors().count(), 0);
}

#[test]
fn branches_share_a_snapshot_and_errors_prevent_writes() {
    let mut app = app();
    let source = model(&mut app, 3, vec![]);
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let n = graph.source(at(source, "number")).unwrap();
    let doubled = graph
        .map("double", &[n], |values| {
            Ok(Some(Box::new(number(&values[0]) * 2)))
        })
        .unwrap();
    let combined = graph
        .map("join", &[n, doubled], |values| {
            Ok(Some(Box::new(number(&values[0]) + number(&values[1]))))
        })
        .unwrap();
    graph.bind(doubled, at(source, "number")).unwrap();
    graph.bind(combined, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 6);
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 9);
    assert_ok(&app);

    let mut graph = BindingGraph::new();
    let n = graph.constant(99_i32).unwrap();
    graph.bind(n, at(target, "number")).unwrap();
    graph
        .map("broken", &[], |_| Err(anyhow!("expected failure")))
        .unwrap();
    app.world_mut()
        .resource_mut::<BindingGraphs>()
        .remove_owner(target);
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 9);
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .next()
            .unwrap()
            .1
            .contains("broken")
    );
}

#[test]
fn ecs_dependencies_and_removals_refresh_without_input_changes() {
    let mut app = app();
    app.insert_resource(Threshold(3));
    let score = app.world_mut().spawn(Score(4)).id();
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let input = graph.constant(1_i32).unwrap();
    let output = graph
        .system(
            "query",
            &[input],
            |In(values): In<BindingInputs>,
             scores: Query<&Score>,
             threshold: Res<Threshold>|
             -> Result<BindingValue> {
                Ok(Some(Box::new(
                    number(&values[0])
                        + scores
                            .iter()
                            .filter(|s| s.0 > threshold.0)
                            .map(|s| s.0)
                            .sum::<i32>(),
                )))
            },
        )
        .unwrap();
    graph.bind(output, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 5);
    app.world_mut().resource_mut::<Threshold>().0 = 5;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 1);
    app.world_mut().get_mut::<Score>(score).unwrap().0 = 8;
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 9);
    app.world_mut().entity_mut(score).remove::<Score>();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 1);
    assert_ok(&app);
}

#[test]
fn list_union_is_stable_and_assignment_shrinks_to_empty() {
    let mut app = app();
    let a = model(&mut app, 0, vec![2, 1, 2]);
    let b = model(&mut app, 0, vec![1, 3]);
    let target = model(&mut app, 0, vec![9; 8]);
    let mut graph = BindingGraph::new();
    let a_node = graph.source(at(a, "items")).unwrap();
    let b_node = graph.source(at(b, "items")).unwrap();
    let union = graph
        .process((a_node, b_node), |a: Vec<i32>, b: Vec<i32>| {
            let mut seen = HashSet::new();
            Ok(a.into_iter().chain(b).filter(|value| seen.insert(*value)).collect::<Vec<_>>())
        })
        .unwrap();
    graph.bind(union, at(target, "items")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(
        app.world().get::<Model>(target).unwrap().items,
        vec![2, 1, 3]
    );
    app.world_mut().get_mut::<Model>(a).unwrap().items.clear();
    app.world_mut().get_mut::<Model>(b).unwrap().items.clear();
    app.update();
    assert!(app.world().get::<Model>(target).unwrap().items.is_empty());
    assert_ok(&app);
}

#[test]
fn list_intersection_and_difference_are_stable_sets() {
    let mut app = app();
    let a = model(&mut app, 0, vec![2, 1, 2, 4]);
    let b = model(&mut app, 0, vec![4, 2, 3]);
    let c = model(&mut app, 0, vec![2, 4, 5]);
    let target = model(&mut app, 0, vec![]);
    let difference_target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let a_node = graph.source(at(a, "items")).unwrap();
    let b_node = graph.source(at(b, "items")).unwrap();
    let c_node = graph.source(at(c, "items")).unwrap();
    let intersection = graph.process((a_node, b_node, c_node), |a: Vec<i32>, b: Vec<i32>, c: Vec<i32>| {
        let mut seen = HashSet::new();
        Ok(a.into_iter().filter(|item| b.contains(item) && c.contains(item) && seen.insert(*item)).collect::<Vec<_>>())
    }).unwrap();
    let difference = graph.process((a_node, b_node), |a: Vec<i32>, b: Vec<i32>| {
        let mut seen = HashSet::new();
        Ok(a.into_iter().filter(|item| !b.contains(item) && seen.insert(*item)).collect::<Vec<_>>())
    }).unwrap();
    graph.bind(intersection, at(target, "items")).unwrap();
    graph.bind(difference, at(difference_target, "items")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().items, vec![2, 4]);
    assert_eq!(
        app.world().get::<Model>(difference_target).unwrap().items,
        vec![1]
    );
    assert_ok(&app);
}

#[test]
fn paths_after_functions_retarget_and_recover_from_missing_records() {
    let mut app = app();
    let source = model(&mut app, 0, vec![]);
    let first = model(&mut app, 7, vec![]);
    let second = model(&mut app, 11, vec![]);
    let target = model(&mut app, 0, vec![]);
    let id = Id::default();
    app.world_mut().get_mut::<Model>(source).unwrap().next = Some(id);
    let mut graph = BindingGraph::new();
    let source_node = graph.source(at(source, "next")).unwrap();
    let identity = graph
        .map(
            "identity",
            &[source_node],
            |mut values| Ok(values.remove(0)),
        )
        .unwrap();
    let output = graph.path(identity, "Model.number").unwrap();
    graph.bind(output, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update(); // unresolved Id retains destination
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 0);
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, first);
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 7);
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, second);
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 11);
    app.world_mut().entity_mut(second).remove::<Model>();
    app.update();
    app.world_mut().entity_mut(second).insert(Model {
        number: 13,
        items: vec![],
        next: None,
    });
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 13);
    assert_ok(&app);
}

#[test]
fn two_way_initializes_propagates_edits_and_rejects_conflicts() {
    let mut app = app();
    let source = model(&mut app, 3, vec![]);
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    graph.two_way(
        at(source, "number"),
        at(target, "number"),
        BindingConflict::Reject,
    );
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 3);
    app.world_mut().get_mut::<Model>(target).unwrap().number = 5;
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 5);
    app.update();
    assert_ok(&app);
    app.world_mut().get_mut::<Model>(source).unwrap().number = 7;
    app.world_mut().get_mut::<Model>(target).unwrap().number = 9;
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 7);
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 9);
    assert!(
        app.world()
            .resource::<BindingGraphs>()
            .errors()
            .next()
            .unwrap()
            .1
            .contains("conflict")
    );
    app.world_mut().get_mut::<Model>(target).unwrap().number = 7;
    app.update();
    assert_ok(&app);
}

#[test]
fn two_way_conflict_policies_are_explicit() {
    for policy in [BindingConflict::SourceWins, BindingConflict::TargetWins] {
        let mut app = app();
        let source = model(&mut app, 0, vec![]);
        let target = model(&mut app, 0, vec![]);
        let mut graph = BindingGraph::new();
        graph.two_way(at(source, "number"), at(target, "number"), policy);
        graph.install(app.world_mut(), target).unwrap();
        app.update();
        app.world_mut().get_mut::<Model>(source).unwrap().number = 1;
        app.world_mut().get_mut::<Model>(target).unwrap().number = 2;
        app.update();
        let expected = if matches!(policy, BindingConflict::SourceWins) {
            1
        } else {
            2
        };
        assert_eq!(app.world().get::<Model>(source).unwrap().number, expected);
        assert_eq!(app.world().get::<Model>(target).unwrap().number, expected);
        assert_ok(&app);
    }
}

#[test]
fn rejects_foreign_nodes_commands_and_bad_paths_and_cleans_up_owners() {
    let mut a = BindingGraph::new();
    let mut b = BindingGraph::new();
    let node = a.constant(1_i32).unwrap();
    assert!(b.process(node, |value: i32| Ok(value)).is_err());
    let mut invalid = BindingGraph::new();
    invalid
        .system(
            "write",
            &[],
            |_: In<BindingInputs>, mut commands: Commands| {
                commands.spawn_empty();
                Ok(None)
            },
        )
        .unwrap();
    assert!(BindingPath::new(Entity::PLACEHOLDER, "Model", Some("items[bad")).is_err());
    let mut app = app();
    let owner = app.world_mut().spawn_empty().id();
    assert!(invalid.install(app.world_mut(), owner).is_err());
    a.install(app.world_mut(), owner).unwrap();
    app.world_mut().despawn(owner);
    app.update();
    assert!(app.world().resource::<BindingGraphs>().graphs.is_empty());
}

#[test]
fn unchanged_values_do_not_mark_destination_changed_and_type_errors_recover() {
    let mut app = app();
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let node = graph.constant(1_i32).unwrap();
    graph.bind(node, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .entity(target)
            .get_ref::<Model>()
            .unwrap()
            .is_changed()
    );
    app.world_mut()
        .resource_mut::<BindingGraphs>()
        .remove_owner(target);
    let mut graph = BindingGraph::new();
    let node = graph.constant("wrong type".to_string()).unwrap();
    graph.bind(node, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().resource::<BindingGraphs>().errors().count(), 1);
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 1);
}

#[test]
fn typed_filters_and_two_way_converters_report_invalid_edits() {
    #[derive(Component, Reflect)]
    struct Text {
        value: String,
    }
    impl Reactive for Text {}
    let mut app = app();
    app.register_component_as::<dyn Reactive, Text>();
    let source = model(&mut app, 12, vec![1, 2, 3]);
    let target = app
        .world_mut()
        .spawn(Text {
            value: String::new(),
        })
        .id();
    let mut graph = BindingGraph::new();
    graph.two_way_with(
        at(source, "number"),
        BindingPath::new(target, "Text", Some("value")).unwrap(),
        BindingConflict::Reject,
        |value: i32| Ok(value.to_string()),
        |value: String| Ok(value.parse::<i32>()?),
    );
    let items = graph.source(at(source, "items")).unwrap();
    let filtered = graph.filter::<i32>(items, |value| *value > 1).unwrap();
    let summed = graph
        .map_value::<Vec<i32>, i32>(filtered, |items| Ok(items.iter().sum()))
        .unwrap();
    let output = model(&mut app, 0, vec![]);
    graph.bind(summed, at(output, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Text>(target).unwrap().value, "12");
    assert_eq!(app.world().get::<Model>(output).unwrap().number, 5);
    app.world_mut().get_mut::<Text>(target).unwrap().value = "invalid".into();
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 12);
    assert_eq!(app.world().resource::<BindingGraphs>().errors().count(), 1);
    app.world_mut().get_mut::<Text>(target).unwrap().value = "42".into();
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 42);
    assert_ok(&app);
}

#[test]
fn two_way_retargeting_starts_a_new_source_authoritative_link() {
    let mut app = app();
    let route = model(&mut app, 0, vec![]);
    let a = model(&mut app, 1, vec![]);
    let b = model(&mut app, 2, vec![]);
    let target = model(&mut app, 0, vec![]);
    let id = Id::default();
    app.world_mut().get_mut::<Model>(route).unwrap().next = Some(id);
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, a);
    let mut graph = BindingGraph::new();
    graph.two_way(
        at(route, "next.Model.number"),
        at(target, "number"),
        BindingConflict::Reject,
    );
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    app.world_mut().get_mut::<Model>(target).unwrap().number = 99;
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, b);
    app.update();
    assert_eq!(app.world().get::<Model>(a).unwrap().number, 1);
    assert_eq!(app.world().get::<Model>(b).unwrap().number, 2);
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 2);
    app.world_mut().get_mut::<Model>(target).unwrap().number = 4;
    app.update();
    assert_eq!(app.world().get::<Model>(b).unwrap().number, 4);
    assert_ok(&app);
}

#[test]
fn checked_macros_cover_fields_options_indices_shapes_and_entity_jumps() {
    use crate::prelude::{path, component_path, property_path};
    #[derive(Component, Reflect)]
    struct Nested {
        inner: Option<Model>,
        r#type: (i32, i32),
    }
    let entity = Entity::PLACEHOLDER;
    assert_eq!(
        path!(entity, Model.number).unwrap(),
        at(entity, "number")
    );
    assert_eq!(
        component_path!(Model.number).unwrap().at(entity),
        at(entity, "number")
    );
    assert_eq!(property_path!(Nested.inner?.items[0]), "inner.items[0]");
    assert_eq!(property_path!(Nested.r#type.0), "type.0");
    assert_eq!(
        property_path!(Model.next -> Model.number),
        "next.Model.number"
    );
    assert_eq!(
        property_path!(crate::prelude::ReactiveView.value as Model.number),
        "value.number"
    );
    assert_eq!(
        property_path!(crate::prelude::ReactiveView.value as Id -> Model.number),
        "value.Model.number"
    );
    let mut calls = 0;
    path!(
        {
            calls += 1;
            entity
        },
        Model.number
    )
    .unwrap();
    assert_eq!(calls, 1);
}

#[test]
fn pipeline_macros_combine_existing_nodes_filters_and_ecs_systems() {
    use crate::prelude::binding_node;
    let mut app = app();
    app.insert_resource(Threshold(3));
    let source = model(&mut app, 0, vec![1, 2, 3, 4]);
    let target = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let filtered = binding_node!(graph;
        source(source, Model.items)
        => filter::<i32>(|value| *value % 2 == 0)
        => map_value::<Vec<i32>, i32>(|values| Ok(values.iter().sum()))
    )
    .unwrap();
    let output = binding_node!(graph;
        node(filtered)
        => system("threshold", |In(values): In<BindingInputs>, threshold: Res<Threshold>| -> Result<BindingValue> {
            Ok(Some(Box::new(number(&values[0]) + threshold.0)))
        })
    ).unwrap();
    graph.bind(output, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 9);
    assert_ok(&app);
}

#[test]
fn pipeline_can_resume_paths_jump_and_extend_through_then() {
    use crate::prelude::binding_node;
    let mut app = app();
    let source = model(&mut app, 12, vec![]);
    let target = model(&mut app, 0, vec![]);
    let id = Id::default();
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, source);
    let mut graph = BindingGraph::new();
    let id_node = graph.constant(id).unwrap();
    let node = binding_node!(graph;
        node(id_node) => jump(Model.number)
        => then(|graph: &mut BindingGraph, node| graph.map_value::<i32, i32>(node, |v| Ok(v + 1)))
    )
    .unwrap();
    graph.bind(node, at(target, "number")).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(target).unwrap().number, 13);
    assert_ok(&app);
}

#[cfg(feature = "bevy_std")]
#[test]
fn builders_preserve_pending_sources_cascades_and_local_edits() {
    use crate::prelude::*;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = app();
    let source = model(&mut app, 7, vec![]);
    let editor = model(&mut app, 0, vec![]);
    let downstream = model(&mut app, 0, vec![]);
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands.entity(editor).builder().bind_component_property(
                source,
                "Model",
                "number",
                "Model",
                "number",
            );
            commands.entity(downstream).builder().bind_from(
                Ok(path!(editor, Model.number).into_binding_expr()),
                component_path!(Model.number),
            );
        })
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(editor).unwrap().number, 7);
    assert_eq!(app.world().get::<Model>(downstream).unwrap().number, 7);
    app.world_mut().get_mut::<Model>(editor).unwrap().number = 99;
    app.update();
    app.update();
    assert_eq!(
        app.world().get::<Model>(editor).unwrap().number,
        99,
        "editing must not be reset by per-frame graph polling"
    );
    assert_eq!(app.world().get::<Model>(downstream).unwrap().number, 99);
    let world = app.world_mut();
    assert!(
        world.resource::<BindingGraphs>().graphs.iter().all(|owned| owned.graph.change_driven())
            && world.resource::<BindingGraphs>().graphs.len() == 2,
        "simple builders are change-driven graphs in the unified runtime"
    );
}

#[cfg(feature = "bevy_std")]
#[test]
fn list_builders_retain_row_callbacks_and_render_changed_snapshots() {
    use crate::prelude::*;
    use bevy::{
        ecs::system::RunSystemOnce,
        reflect::{DynamicStruct, List},
    };
    #[derive(Component, Reflect, Clone)]
    struct Rows {
        items: Vec<Model>,
    }
    impl Reactive for Rows {}
    #[derive(Component)]
    struct RowBuilt;
    let mut app = app();
    app.register_component_as::<dyn Reactive, Rows>();
    app.register_component_as::<dyn Reactive, ReactiveListView>();
    app.register_component_as::<dyn Reactive, ReactiveView>();
    app.add_systems(Update, process_reactive_lists);
    let source = app
        .world_mut()
        .spawn(Rows {
            items: vec![
                Model {
                    number: 1,
                    items: vec![],
                    next: None,
                },
                Model {
                    number: 2,
                    items: vec![],
                    next: None,
                },
            ],
        })
        .id();
    let legacy = app.world_mut().spawn_empty().id();
    let checked = app.world_mut().spawn_empty().id();
    let computed = app.world_mut().spawn_empty().id();
    let expression = app.world_mut().spawn_empty().id();
    app.world_mut().run_system_once(move |mut commands: Commands| {
        let row = |In(entity): In<Entity>, mut commands: Commands| { commands.entity(entity).insert(RowBuilt); };
        commands.entity(legacy).builder().bind_list(source, "Rows", "items", row);
        commands.entity(checked).builder().bind_list_from(path!(source, Rows.items), row);
        let mut graph = BindingGraph::new();
        let node = binding_node!(graph; source(source, Rows.items) => filter::<Model>(|row| row.number > 1)).unwrap();
        commands.entity(computed).builder().bind_list_node(graph, node, row).unwrap();
        commands.entity(expression).builder().bind_list_from(
            process(path!(source, Rows.items), |rows: Vec<Model>| {
                Ok(rows.into_iter().filter(|row| row.number > 1).collect::<Vec<_>>())
            }),
            row,
        );
    }).unwrap();
    app.update();
    app.update(); // computed list is published in PostUpdate
    for (entity, count) in [(legacy, 2), (checked, 2), (computed, 1), (expression, 1)] {
        let children = app.world().get::<Children>(entity).unwrap();
        assert_eq!(children.len(), count);
        for child in children.iter() {
            assert!(app.world().get::<RowBuilt>(child).is_some());
        }
    }
    app.world_mut()
        .get_mut::<Rows>(source)
        .unwrap()
        .items
        .clear();
    app.update();
    app.update();
    for entity in [legacy, checked, computed, expression] {
        assert!(
            app.world()
                .get::<ReactiveListView>(entity)
                .unwrap()
                .value
                .is_empty()
        );
        assert!(
            app.world()
                .get::<Children>(entity)
                .is_none_or(|children| children.is_empty())
        );
    }
    assert_ok(&app);
}

#[test]
fn scalar_list_rows_are_readable_in_callbacks_and_refresh() {
    use crate::prelude::*;
    use bevy::ecs::system::RunSystemOnce;

    #[derive(Component)]
    struct RowValue(i32);
    let mut app = app();
    app.register_component_as::<dyn Reactive, ReactiveListView>();
    app.register_component_as::<dyn Reactive, ReactiveView>();
    app.add_systems(Update, process_reactive_lists);
    let source = model(&mut app, 0, vec![3, 3, 7]);
    let owners = std::array::from_fn::<_, 3, _>(|_| app.world_mut().spawn_empty().id());
    app.world_mut().run_system_once(move |mut commands: Commands| {
        let row = |In(entity): In<Entity>, views: Query<&ReactiveView>, mut commands: Commands| {
            let view = views.get(entity).unwrap();
            let item = i32::from_reflect(view.value.as_ref()).unwrap();
            commands.entity(entity).insert(RowValue(item));
            commands.entity(entity).insert(Model { number: 0, items: vec![], next: None });
            commands.entity(entity).builder().bind_from(
                path!(entity, ReactiveView.value), component_path!(Model.number));
        };
        commands.entity(owners[0]).builder().bind_list(source, "Model", "items", row);
        commands.entity(owners[1]).builder().bind_list_from(path!(source, Model.items), row);
        let mut graph = BindingGraph::new();
        let node = graph.source(at(source, "items")).unwrap();
        commands.entity(owners[2]).builder().bind_list_node(graph, node, row).unwrap();
    }).unwrap();

    let mut previous_children = Vec::new();
    for expected in [vec![3, 3, 7], vec![9], vec![]] {
        app.world_mut().get_mut::<Model>(source).unwrap().items = expected.clone();
        app.update();
        app.update();
        app.update(); // row-local direct bindings run after the renderer's commands
        for old in previous_children.drain(..) {
            assert!(app.world().get_entity(old).is_err());
        }
        for owner in owners {
            let children: Vec<_> = app.world().get::<Children>(owner)
                .map(|children| children.iter().collect()).unwrap_or_default();
            let values: Vec<_> = children.iter()
                .map(|child| app.world().get::<RowValue>(*child).unwrap().0).collect();
            assert_eq!(values, expected);
            for (child, expected) in children.iter().zip(&expected) {
                assert_eq!(app.world().get::<Model>(*child).unwrap().number, *expected);
            }
            previous_children.extend(children);
        }
        assert_ok(&app);
    }
}

#[test]
fn uuid_set_minus_map_keys_renders_scalar_rows() {
    use crate::prelude::*;
    use bevy::ecs::system::RunSystemOnce;
    use uuid::Uuid;

    #[derive(Component, Reflect, Clone)]
    struct Networks {
        available: HashSet<Uuid>,
        configured: std::collections::HashMap<Uuid, String>,
    }
    impl Reactive for Networks {}
    #[derive(Component)]
    struct NetworkRow(Uuid);
    let mut app = app();
    app.register_component_as::<dyn Reactive, Networks>();
    app.register_component_as::<dyn Reactive, ReactiveListView>();
    app.register_component_as::<dyn Reactive, ReactiveView>();
    app.add_systems(Update, process_reactive_lists);
    let a = Uuid::from_u128(1);
    let b = Uuid::from_u128(2);
    let source = app.world_mut().spawn(Networks {
        available: HashSet::from([a, b]),
        configured: std::collections::HashMap::from([(a, "saved".into())]),
    }).id();
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut().run_system_once(move |mut commands: Commands| {
        let mut graph = BindingGraph::new();
        let available = graph.source(path!(source, Networks.available).unwrap()).unwrap();
        let configured = graph.source(path!(source, Networks.configured).unwrap()).unwrap();
        let difference = graph.process((available, configured),
            |available: HashSet<Uuid>, configured: std::collections::HashMap<Uuid, String>| {
                let mut ids: Vec<_> = available.into_iter().filter(|id| !configured.contains_key(id)).collect();
                ids.sort_unstable();
                Ok(ids)
            }).unwrap();
        commands.entity(owner).builder().bind_list_node(graph, difference,
            |In(row): In<Entity>, views: Query<&ReactiveView>, mut commands: Commands| {
                let view = views.get(row).unwrap();
                let id = Uuid::from_reflect(view.value.as_ref()).unwrap();
                commands.entity(row).insert(NetworkRow(id));
            }).unwrap();
    }).unwrap();
    app.update();
    app.update();
    let children = app.world().get::<Children>(owner).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(app.world().get::<NetworkRow>(children[0]).unwrap().0, b);
    app.world_mut().get_mut::<Networks>(source).unwrap().configured.insert(b, "saved too".into());
    app.update();
    app.update();
    assert!(app.world().get::<Children>(owner).is_none_or(|children| children.is_empty()));
    assert_ok(&app);
}

#[test]
fn dynamic_view_paths_read_write_and_retarget_records() {
    use crate::prelude::*;
    let mut app = app();
    app.register_component_as::<dyn Reactive, ReactiveView>();
    let first = model(&mut app, 7, vec![]);
    let second = model(&mut app, 11, vec![]);
    let first_id = Id::new();
    let second_id = Id::new();
    app.world_mut().resource_mut::<DBConfig>().insert_entity(&first_id, first);
    app.world_mut().resource_mut::<DBConfig>().insert_entity(&second_id, second);
    let view = app.world_mut().spawn(ReactiveView { value: Dynamic::new(&first_id) }).id();
    let destination = model(&mut app, 0, vec![]);
    let mut graph = BindingGraph::new();
    let source = graph.source(path!(view, ReactiveView.value as Id -> Model.number).unwrap()).unwrap();
    graph.bind(source, at(destination, "number")).unwrap();
    graph.install(app.world_mut(), view).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(destination).unwrap().number, 7);
    app.world_mut().get_mut::<ReactiveView>(view).unwrap().value = Dynamic::new(&second_id);
    app.update();
    assert_eq!(app.world().get::<Model>(destination).unwrap().number, 11);

    let mut graph = BindingGraph::new();
    let value = graph.constant(23_i32).unwrap();
    graph.bind(value, path!(view, ReactiveView.value as Id -> Model.number).unwrap()).unwrap();
    graph.install(app.world_mut(), view).unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(first).unwrap().number, 7);
    assert_eq!(app.world().get::<Model>(second).unwrap().number, 23);

    app.world_mut().resource_mut::<BindingGraphs>().remove_owner(view);
    // The same field also supports a nested struct, without an entity jump.
    app.world_mut().get_mut::<ReactiveView>(view).unwrap().value = Dynamic::new(&Model {
        number: 0, items: vec![], next: None,
    });
    let mut graph = BindingGraph::new();
    let value = graph.constant(31_i32).unwrap();
    graph.bind(value, path!(view, ReactiveView.value as Model.number).unwrap()).unwrap();
    graph.install(app.world_mut(), view).unwrap();
    app.update();
    let value = app.world().get::<ReactiveView>(view).unwrap();
    assert_eq!(Model::from_reflect(value.value.as_ref()).unwrap().number, 31);
    assert_ok(&app);
}

mod typed;
