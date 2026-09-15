use super::*;
use crate::prelude::*;
use bevy::{ecs::system::RunSystemOnce, reflect::Map};
use std::collections::HashMap;

#[derive(Component, Reflect, Clone)]
struct MapSource {
    entries: HashMap<String, Vec<i32>>,
}
impl Reactive for MapSource {}

#[derive(Component)]
struct Seen(String, Vec<i32>);

fn render(In(row): In<Entity>, rows: Query<(&ReactiveMapKey, &ReactiveView)>, mut commands: Commands) {
    let (key, value) = rows.get(row).unwrap();
    commands.entity(row).insert(Seen(
        String::from_reflect(key.value.as_ref()).unwrap(),
        Vec::<i32>::from_reflect(value.value.as_ref()).unwrap(),
    ));
    // Nested collections are an explicit rendering choice; the row retains its value slot.
    commands.entity(row).builder().bind_list_from(path!(row, ReactiveView.value),
        |In(child): In<Entity>, values: Query<&ReactiveView>| {
            assert!(i32::from_reflect(values.get(child).unwrap().value.as_ref()).is_some());
        });
}

#[test]
fn map_builders_render_keys_values_and_replace_nested_snapshots() {
    let mut app = app();
    app.register_component_as::<dyn Reactive, MapSource>();
    app.add_reactive::<ReactiveMapView>().add_reactive::<ReactiveMapKey>()
        .add_reactive::<ReactiveView>().add_reactive::<ReactiveListView>();
    app.add_systems(Update, (process_reactive_maps, process_reactive_lists));
    let source = app.world_mut().spawn(MapSource { entries: HashMap::new() }).id();
    let owners: [_; 4] = std::array::from_fn(|_| app.world_mut().spawn_empty().id());
    app.world_mut().run_system_once(move |mut commands: Commands| {
        commands.entity(owners[0]).builder().bind_map(source, "MapSource", "entries", render);
        commands.entity(owners[1]).builder().bind_map_from(path!(source, MapSource.entries), render);
        commands.entity(owners[2]).builder().bind_map_from(
            process(path!(source, MapSource.entries), |map: HashMap<String, Vec<i32>>| Ok(map)), render);
        let mut graph = BindingGraph::new();
        let node = graph.source(path!(source, MapSource.entries).unwrap()).unwrap();
        commands.entity(owners[3]).builder().bind_map_node(graph, node, render).unwrap();
    }).unwrap();

    let mut previous = Vec::new();
    for expected in [
        HashMap::from([("a".to_string(), vec![1, 2]), ("b".to_string(), vec![3])]),
        HashMap::from([("a".to_string(), vec![9])]),
        HashMap::new(),
    ] {
        app.world_mut().get_mut::<MapSource>(source).unwrap().entries = expected.clone();
        for _ in 0..5 { app.update(); }
        for old in previous.drain(..) { assert!(app.world().get_entity(old).is_err()); }
        for owner in owners {
            let view = app.world().get::<ReactiveMapView>(owner).unwrap();
            assert_eq!(HashMap::<String, Vec<i32>>::from_reflect(&view.value), Some(expected.clone()));
            let children: Vec<_> = app.world().get::<Children>(owner)
                .map(|children| children.iter().collect()).unwrap_or_default();
            let actual: HashMap<_, _> = children.iter().map(|child| {
                let seen = app.world().get::<Seen>(*child).unwrap();
                let nested: Vec<_> = app.world().get::<Children>(*child).unwrap().iter().map(|leaf| {
                    i32::from_reflect(app.world().get::<ReactiveView>(leaf).unwrap().value.as_ref()).unwrap()
                }).collect();
                assert_eq!(nested, seen.1);
                (seen.0.clone(), seen.1.clone())
            }).collect();
            assert_eq!(actual, expected);
            previous.extend(children);
        }
        assert_ok(&app);
    }
}

#[test]
fn map_view_round_trips_and_unconfigured_views_do_not_panic() {
    let expected = HashMap::from([("key".to_string(), "value".to_string())]);
    let view = ReactiveMapView { value: expected.to_dynamic_map(), create_entity_func: None };
    let cloned = ReactiveMapView::from_reflect(view.clone_value().as_ref()).unwrap();
    let json = serde_json::to_string(&view).unwrap();
    let decoded: ReactiveMapView = serde_json::from_str(&json).unwrap();
    let bytes = postcard::to_allocvec(&view).unwrap();
    let binary: ReactiveMapView = postcard::from_bytes(&bytes).unwrap();
    for copy in [cloned, decoded, binary] {
        assert_eq!(HashMap::<String, String>::from_reflect(&copy.value), Some(expected.clone()));
        assert!(copy.create_entity_func.is_none());
    }
    let mut app = app();
    app.add_systems(Update, process_reactive_maps);
    app.world_mut().spawn(view);
    app.update();
}

#[test]
fn invalid_map_expression_has_no_partial_attachment() {
    let mut app = app();
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut().run_system_once(move |mut commands: Commands| {
        assert!(commands.entity(owner).builder().try_bind_map_from(
            Err::<BindingPath, _>(anyhow!("bad source")), render).is_err());
    }).unwrap();
    assert!(app.world().get::<ReactiveMapView>(owner).is_none());
    assert!(app.world().resource::<BindingGraphs>().graphs.is_empty());
}

#[test]
fn removing_one_map_entry_preserves_other_editors_without_rendering_them_again() {
    #[derive(Resource, Default)]
    struct Renders(usize);
    #[derive(Component)]
    struct Draft(String);

    let mut app = app();
    app.register_component_as::<dyn Reactive, MapSource>();
    app.add_reactive::<ReactiveMapView>()
        .add_reactive::<ReactiveMapKey>()
        .add_reactive::<ReactiveView>();
    app.init_resource::<Renders>();
    app.add_systems(PostUpdate, process_reactive_maps.after(BindingGraphSet));
    let source = app.world_mut().spawn(MapSource {
        entries: (0..32).map(|i| (i.to_string(), vec![i])).collect(),
    }).id();
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut().run_system_once(move |mut commands: Commands| {
        commands.entity(owner).builder().bind_map_from(
            path!(source, MapSource.entries),
            |In(row): In<Entity>, mut commands: Commands, mut renders: ResMut<Renders>| {
                renders.0 += 1;
                commands.entity(row).with_children(|parent| {
                    parent.spawn(Draft("unsaved credentials".into()));
                });
            },
        );
    }).unwrap();
    app.update();
    let rows: HashMap<String, Entity> = app.world().get::<Children>(owner).unwrap().iter()
        .map(|row| (String::from_reflect(app.world().get::<ReactiveMapKey>(row).unwrap().value.as_ref()).unwrap(), row))
        .collect();
    assert_eq!(rows.len(), 32);
    assert_eq!(app.world().resource::<Renders>().0, 32);
    let editor = app.world().get::<Children>(rows["1"]).unwrap()[0];

    // Saving one network filters only that key out of the available-map snapshot.
    app.world_mut().get_mut::<MapSource>(source).unwrap().entries.remove("0");
    app.update();
    assert!(app.world().get_entity(rows["0"]).is_err());
    assert_eq!(app.world().get::<Children>(owner).unwrap().len(), 31);
    assert_eq!(app.world().resource::<Renders>().0, 32);
    for (key, row) in &rows {
        if key != "0" { assert!(app.world().get_entity(*row).is_ok()); }
    }
    assert_eq!(app.world().get::<Draft>(editor).unwrap().0, "unsaved credentials");

    // A changed value still rebuilds its snapshot callback; unrelated rows survive.
    app.world_mut().get_mut::<MapSource>(source).unwrap().entries.insert("2".into(), vec![99]);
    app.update();
    assert!(app.world().get_entity(rows["2"]).is_err());
    assert_eq!(app.world().resource::<Renders>().0, 33);
    assert!(app.world().get::<Draft>(editor).is_some());
    assert_ok(&app);
}
