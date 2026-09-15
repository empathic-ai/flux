use super::*;
use crate::prelude::InputField;

#[derive(Reflect, Clone, PartialEq, Debug)]
struct Item {
    id: u32,
    text: String,
}
#[derive(Component, Reflect, Clone, PartialEq, Default)]
struct Model {
    items: Vec<Item>,
    map: HashMap<uuid::Uuid, Item>,
    number: i32,
    next: Option<Id>,
}
impl Reactive for Model {}
fn item(id: u32) -> Item {
    Item {
        id,
        text: format!("item {id}"),
    }
}
fn app() -> App {
    let mut app = App::new();
    app.add_plugins(EditBindingPlugin);
    app.register_component_as::<dyn Reactive, Model>();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(futures::lock::Mutex::new(surrealdb::Surreal::init())),
        id_mappings: default(),
        entity_mappings: default(),
    });
    app
}
fn install<A: EditCollection>(
    app: &mut App,
    source: Entity,
    path: &str,
    adapter: A,
    policy: EditPolicy,
) -> Entity {
    let owner = app.world_mut().spawn_empty().id();
    install_edit_binding(
        app.world_mut(),
        owner,
        BindingPath::new(source, "Model", Some(path)).unwrap(),
        adapter,
        policy,
        EditRenderer::new(|In(_): In<Entity>| {}),
        |_| true,
    )
    .unwrap();
    app.update();
    owner
}
fn row(app: &mut App, id: u32) -> Entity {
    let mut query = app.world_mut().query::<(Entity, &EditSession<Item>)>();
    query
        .iter(app.world())
        .find(|(_, s)| s.value().id == id)
        .unwrap()
        .0
}
fn edit(app: &mut App, row: Entity, text: &str, save: bool) {
    let mut session = app.world_mut().get_mut::<EditSession<Item>>(row).unwrap();
    session.begin().unwrap();
    session.edit(|v| v.text = text.into()).unwrap();
    if save {
        session.save().unwrap();
    }
}
#[test]
fn uuid_map_commit_preserves_other_drafts_and_updates_only_original_value() {
    let mut app = app();
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let source = app
        .world_mut()
        .spawn(Model {
            map: HashMap::from([(a, item(1)), (b, item(2))]),
            ..default()
        })
        .id();
    install(
        &mut app,
        source,
        "map",
        MapEntries::<uuid::Uuid, Item>::default(),
        EditPolicy::Manual,
    );
    let r1 = row(&mut app, 1);
    let r2 = row(&mut app, 2);
    edit(&mut app, r2, "unsaved", false);
    edit(&mut app, r1, "saved", true);
    app.update();
    assert_eq!(
        app.world().get::<Model>(source).unwrap().map[&a].text,
        "saved"
    );
    assert_eq!(app.world().get::<Model>(source).unwrap().map[&b], item(2));
    assert_eq!(row(&mut app, 2), r2);
    assert_eq!(
        app.world()
            .get::<EditSession<Item>>(r2)
            .unwrap()
            .value()
            .text,
        "unsaved"
    );
    assert!(
        app.world()
            .get::<EditSession<Item>>(r2)
            .unwrap()
            .is_editing()
    );
    app.world_mut()
        .get_mut::<EditSession<Item>>(r2)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<Model>(source).unwrap().map[&b].text,
        "unsaved"
    );
}
#[test]
fn begin_freezes_even_untouched_draft_and_cancel_reloads_source() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1)],
            ..default()
        })
        .id();
    install(
        &mut app,
        source,
        "items",
        KeyedVec::new(|v: &Item| v.id),
        EditPolicy::Manual,
    );
    let r = row(&mut app, 1);
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .begin()
        .unwrap();
    app.world_mut().get_mut::<Model>(source).unwrap().items[0].text = "remote".into();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().value(),
        &item(1)
    );
    assert!(app.world().get::<EditStatus>(r).unwrap().stale);
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().error(),
        Some(EditError::SourceChanged)
    );
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .cancel();
    app.update();
    assert_eq!(
        app.world()
            .get::<EditSession<Item>>(r)
            .unwrap()
            .value()
            .text,
        "remote"
    );
    assert!(!app.world().get::<EditStatus>(r).unwrap().active);
}
#[test]
fn keyed_reordering_preserves_entity_and_missing_item_retains_active_draft() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1), item(2)],
            ..default()
        })
        .id();
    let owner = install(
        &mut app,
        source,
        "items",
        KeyedVec::new(|v: &Item| v.id),
        EditPolicy::Manual,
    );
    let r = row(&mut app, 1);
    edit(&mut app, r, "draft", false);
    app.world_mut()
        .get_mut::<Model>(source)
        .unwrap()
        .items
        .reverse();
    app.update();
    assert_eq!(row(&mut app, 1), r);
    assert_eq!(app.world().get::<Children>(owner).unwrap()[1], r);
    app.world_mut()
        .get_mut::<Model>(source)
        .unwrap()
        .items
        .retain(|v| v.id != 1);
    app.update();
    assert!(app.world().get::<EditStatus>(r).unwrap().missing);
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().error(),
        Some(EditError::Missing)
    );
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .cancel();
    app.update();
    assert!(app.world().get_entity(r).is_err());
}
#[test]
fn positional_edits_reject_reordering_instead_of_writing_wrong_item() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1), item(2)],
            ..default()
        })
        .id();
    install(
        &mut app,
        source,
        "items",
        IndexedVec::<Item>::default(),
        EditPolicy::Manual,
    );
    let r = row(&mut app, 1);
    edit(&mut app, r, "draft", false);
    app.world_mut()
        .get_mut::<Model>(source)
        .unwrap()
        .items
        .reverse();
    app.update();
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().error(),
        Some(EditError::CollectionChanged)
    );
    assert_eq!(
        app.world().get::<Model>(source).unwrap().items,
        vec![item(2), item(1)]
    );
}
#[test]
fn duplicate_keys_block_commits_and_key_edits_are_rejected() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1)],
            ..default()
        })
        .id();
    let owner = install(
        &mut app,
        source,
        "items",
        KeyedVec::new(|v: &Item| v.id),
        EditPolicy::Manual,
    );
    let r = row(&mut app, 1);
    edit(&mut app, r, "draft", false);
    app.world_mut()
        .get_mut::<Model>(source)
        .unwrap()
        .items
        .push(item(1));
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().error(),
        Some(EditError::DuplicateKey)
    );
    assert!(
        !app.world()
            .get::<EditStatus>(owner)
            .unwrap()
            .message
            .is_empty()
    );
    app.world_mut()
        .get_mut::<Model>(source)
        .unwrap()
        .items
        .pop();
    let mut session = app.world_mut().get_mut::<EditSession<Item>>(r).unwrap();
    session.edit(|v| v.id = 4).unwrap();
    session.save().unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().error(),
        Some(EditError::Invalid)
    );
}
#[test]
fn retargeted_id_does_not_redirect_an_active_save() {
    let mut app = app();
    let a = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1)],
            ..default()
        })
        .id();
    let b = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1)],
            ..default()
        })
        .id();
    let id = Id::default();
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, a);
    let route = app
        .world_mut()
        .spawn(Model {
            next: Some(id),
            ..default()
        })
        .id();
    install(
        &mut app,
        route,
        "next.Model.items",
        KeyedVec::new(|v: &Item| v.id),
        EditPolicy::Manual,
    );
    let r = row(&mut app, 1);
    edit(&mut app, r, "draft", false);
    app.world_mut()
        .resource_mut::<DBConfig>()
        .insert_entity(&id, b);
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<Item>>(r).unwrap().error(),
        Some(EditError::SourceChanged)
    );
    assert_eq!(app.world().get::<Model>(a).unwrap().items[0], item(1));
    assert_eq!(app.world().get::<Model>(b).unwrap().items[0], item(1));
}
#[test]
fn input_adapter_enforces_readonly_and_manual_save_cancel_and_live_edits() {
    for policy in [
        EditPolicy::DisplayOnly,
        EditPolicy::Manual,
        EditPolicy::Live,
    ] {
        let mut app = app();
        let source = app
            .world_mut()
            .spawn(Model {
                number: 1,
                ..default()
            })
            .id();
        let owner = install(
            &mut app,
            source,
            "number",
            EditValue::<i32>::default(),
            policy,
        );
        let r = app.world().get::<Children>(owner).unwrap()[0];
        let input = app.world_mut().spawn(InputField::default()).id();
        install_edit_input(
            app.world_mut(),
            input,
            r,
            |v: &i32| v.to_string(),
            |v, text| *v = text.parse().unwrap(),
        )
        .unwrap();
        if policy == EditPolicy::DisplayOnly {
            assert_eq!(
                app.world_mut()
                    .get_mut::<EditSession<i32>>(r)
                    .unwrap()
                    .begin(),
                Err(EditError::ReadOnly)
            );
        } else {
            app.world_mut()
                .get_mut::<EditSession<i32>>(r)
                .unwrap()
                .begin()
                .unwrap();
        }
        app.world_mut().get_mut::<InputField>(input).unwrap().text = "2".into();
        app.update();
        assert_eq!(
            app.world().get::<Model>(source).unwrap().number,
            if policy == EditPolicy::Live { 2 } else { 1 }
        );
        assert_eq!(
            app.world().get::<InputField>(input).unwrap().read_only,
            policy == EditPolicy::DisplayOnly
        );
        if policy == EditPolicy::Manual {
            app.world_mut()
                .get_mut::<EditSession<i32>>(r)
                .unwrap()
                .save()
                .unwrap();
            app.update();
            assert_eq!(app.world().get::<Model>(source).unwrap().number, 2);
            app.world_mut()
                .get_mut::<EditSession<i32>>(r)
                .unwrap()
                .begin()
                .unwrap();
            app.world_mut().get_mut::<InputField>(input).unwrap().text = "3".into();
            app.update();
            app.world_mut()
                .get_mut::<EditSession<i32>>(r)
                .unwrap()
                .cancel();
            app.update();
            assert_eq!(app.world().get::<InputField>(input).unwrap().text, "2");
        }
    }
}
#[test]
fn validation_failure_keeps_draft_and_owner_despawn_releases_driver() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            number: 1,
            ..default()
        })
        .id();
    let owner = app.world_mut().spawn_empty().id();
    install_edit_binding(
        app.world_mut(),
        owner,
        BindingPath::new(source, "Model", Some("number")).unwrap(),
        EditValue::<i32>::default(),
        EditPolicy::Manual,
        EditRenderer::new(|In(_): In<Entity>| {}),
        |v| *v >= 0,
    )
    .unwrap();
    app.update();
    let r = app.world().get::<Children>(owner).unwrap()[0];
    let mut s = app.world_mut().get_mut::<EditSession<i32>>(r).unwrap();
    s.begin().unwrap();
    s.edit(|v| *v = -1).unwrap();
    s.save().unwrap();
    app.update();
    assert_eq!(
        app.world().get::<EditSession<i32>>(r).unwrap().error(),
        Some(EditError::Invalid)
    );
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 1);
    app.world_mut().despawn(owner);
    app.update();
    assert!(app.world().resource::<EditBindings>().drivers.is_empty());
}

#[test]
fn filtered_sorted_edits_keep_original_keys_and_hidden_active_rows() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1), item(2), item(3)],
            ..default()
        })
        .id();
    let visible = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = visible.clone();
    let adapter = KeyedVec::new(|v: &Item| v.id)
        .filter(move |v: &Item| v.id != 2 && flag.load(std::sync::atomic::Ordering::Relaxed))
        .sort_by(|a: &Item, b: &Item| b.id.cmp(&a.id));
    let owner = install(&mut app, source, "items", adapter, EditPolicy::Manual);
    let r = row(&mut app, 3);
    assert_eq!(app.world().get::<Children>(owner).unwrap()[0], r);
    edit(&mut app, r, "draft", false);
    visible.store(false, std::sync::atomic::Ordering::Relaxed);
    app.update();
    assert!(app.world().get_entity(r).is_ok());
    assert!(!app.world().get::<EditStatus>(r).unwrap().missing);
    app.world_mut()
        .get_mut::<EditSession<Item>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(
        app.world().get::<Model>(source).unwrap().items[2].text,
        "draft"
    );
    assert_eq!(app.world().get::<Model>(source).unwrap().items[0], item(1));
    app.update();
    assert!(app.world().get_entity(r).is_err());
}

#[test]
fn map_order_is_stable_and_commit_observers_see_reinserted_sessions() {
    #[derive(Resource, Default)]
    struct Seen(u64);
    let mut app = app();
    app.init_resource::<Seen>();
    let source = app
        .world_mut()
        .spawn(Model {
            map: (1..8).map(|i| (uuid::Uuid::new_v4(), item(i))).collect(),
            ..default()
        })
        .id();
    let owner = install(
        &mut app,
        source,
        "map",
        MapEntries::<uuid::Uuid, Item>::default(),
        EditPolicy::Manual,
    );
    let order: Vec<Entity> = app.world().get::<Children>(owner).unwrap().iter().collect();
    for _ in 0..4 {
        app.update();
        assert_eq!(
            app.world()
                .get::<Children>(owner)
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            order
        );
    }
    let r = row(&mut app, 1);
    app.world_mut().entity_mut(r).observe(
        |ev: Trigger<EditCommitted>,
         sessions: Query<&EditSession<Item>>,
         mut seen: ResMut<Seen>| {
            let session = sessions.get(ev.row).unwrap();
            assert_eq!(session.value().text, "committed");
            assert_eq!(session.commits(), ev.sequence);
            seen.0 += 1;
        },
    );
    edit(&mut app, r, "committed", true);
    app.update();
    app.world_mut().flush();
    assert_eq!(app.world().resource::<Seen>().0, 1);
}

#[test]
fn invalid_text_survives_source_refresh_blocks_save_and_cancel_reloads() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            number: 1,
            ..default()
        })
        .id();
    let owner = install(
        &mut app,
        source,
        "number",
        EditValue::<i32>::default(),
        EditPolicy::Manual,
    );
    let r = app.world().get::<Children>(owner).unwrap()[0];
    let input = app.world_mut().spawn(InputField::default()).id();
    install_edit_input_try(
        app.world_mut(),
        input,
        r,
        |v: &i32| v.to_string(),
        |v, text| {
            *v = text.parse().map_err(|_| EditError::Invalid)?;
            Ok(())
        },
    )
    .unwrap();
    app.world_mut()
        .get_mut::<EditSession<i32>>(r)
        .unwrap()
        .begin()
        .unwrap();
    app.world_mut().get_mut::<InputField>(input).unwrap().text = "unfinished-".into();
    app.update();
    app.world_mut()
        .get_mut::<EditSession<i32>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 1);
    assert_eq!(
        app.world().get::<EditSession<i32>>(r).unwrap().error(),
        Some(EditError::Invalid)
    );
    assert_eq!(
        app.world().get::<InputField>(input).unwrap().text,
        "unfinished-"
    );
    app.world_mut().get_mut::<Model>(source).unwrap().number = 5;
    app.update();
    assert_eq!(
        app.world().get::<InputField>(input).unwrap().text,
        "unfinished-"
    );
    assert!(
        !app.world()
            .get::<EditInputStatus>(input)
            .unwrap()
            .message
            .is_empty()
    );
    app.world_mut()
        .get_mut::<EditSession<i32>>(r)
        .unwrap()
        .cancel();
    app.update();
    assert_eq!(app.world().get::<InputField>(input).unwrap().text, "5");
    assert!(
        app.world()
            .get::<EditInputStatus>(input)
            .unwrap()
            .message
            .is_empty()
    );
    app.world_mut()
        .get_mut::<EditSession<i32>>(r)
        .unwrap()
        .begin()
        .unwrap();
    app.world_mut().get_mut::<InputField>(input).unwrap().text = "6".into();
    app.world_mut()
        .get_mut::<EditSession<i32>>(r)
        .unwrap()
        .save()
        .unwrap();
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 6);
}

#[test]
fn live_inputs_follow_source_between_edits_but_keep_invalid_text() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            number: 1,
            ..default()
        })
        .id();
    let owner = install(
        &mut app,
        source,
        "number",
        EditValue::<i32>::default(),
        EditPolicy::Live,
    );
    let r = app.world().get::<Children>(owner).unwrap()[0];
    let input = app.world_mut().spawn(InputField::default()).id();
    install_edit_input_try(
        app.world_mut(),
        input,
        r,
        |v: &i32| v.to_string(),
        |v, text| {
            *v = text.parse().map_err(|_| EditError::Invalid)?;
            Ok(())
        },
    )
    .unwrap();
    app.world_mut().get_mut::<InputField>(input).unwrap().text = "2".into();
    app.update();
    assert_eq!(app.world().get::<Model>(source).unwrap().number, 2);
    app.world_mut().get_mut::<Model>(source).unwrap().number = 3;
    app.update();
    assert_eq!(app.world().get::<InputField>(input).unwrap().text, "3");
    app.world_mut().get_mut::<InputField>(input).unwrap().text = "-".into();
    app.update();
    app.world_mut().get_mut::<Model>(source).unwrap().number = 4;
    app.update();
    assert_eq!(app.world().get::<InputField>(input).unwrap().text, "-");
    app.world_mut()
        .get_mut::<EditSession<i32>>(r)
        .unwrap()
        .cancel();
    app.update();
    assert_eq!(app.world().get::<InputField>(input).unwrap().text, "4");
    assert!(!app.world().get::<InputField>(input).unwrap().read_only);
}

#[test]
fn application_commit_observers_cannot_be_overwritten_by_a_later_editor_snapshot() {
    let mut app = app();
    let source = app
        .world_mut()
        .spawn(Model {
            items: vec![item(1), item(2), item(3)],
            ..default()
        })
        .id();
    let a = install(
        &mut app,
        source,
        "items",
        KeyedVec::new(|v: &Item| v.id),
        EditPolicy::Manual,
    );
    let b = install(
        &mut app,
        source,
        "items",
        KeyedVec::new(|v: &Item| v.id),
        EditPolicy::Manual,
    );
    let (first, second) = if a.to_bits() < b.to_bits() {
        (a, b)
    } else {
        (b, a)
    };
    let r1 = app.world().get::<Children>(first).unwrap()[0];
    let r2 = app.world().get::<Children>(second).unwrap()[1];
    app.world_mut().entity_mut(r1).observe(
        move |_: Trigger<EditCommitted>, mut models: Query<&mut Model>| {
            models.get_mut(source).unwrap().items[2].text = "domain action".into();
        },
    );
    edit(&mut app, r1, "first save", true);
    edit(&mut app, r2, "second save", true);
    app.update();
    app.world_mut().flush();
    let items = &app.world().get::<Model>(source).unwrap().items;
    assert_eq!(items[0].text, "first save");
    assert_eq!(items[1].text, "second save");
    assert_eq!(items[2].text, "domain action");
}
