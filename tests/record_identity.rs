use bevy::prelude::*;
use flux::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Reflect, Reactive, Clone, Debug, Serialize, Deserialize)]
struct PublicRecord { value: u32 }

#[derive(Component, Reflect, Reactive, Clone, Debug, Serialize, Deserialize)]
struct PrivateRecord { value: u32 }

#[derive(Resource, Default)]
struct LoadedEntities(Vec<Entity>);

#[cfg(feature = "client")]
#[derive(Component, Reflect, Reactive, Clone)]
struct RecordReference { record: Id }

#[cfg(feature = "client")]
#[test]
fn binding_requests_missing_record_once_and_refreshes_after_delivery() {
    let mut app = App::new();
    app.add_plugins(BindingGraphPlugin)
        .insert_resource(DBConfig {
            #[cfg(feature = "surrealdb")]
            db: std::sync::Arc::new(surrealdb::Surreal::init()),
            id_mappings: Default::default(),
            entity_mappings: Default::default(),
        })
        .insert_resource(Session::new(Id::new()));
    use bevy_trait_query::RegisterExt;
    app.register_component_as::<dyn Reactive, RecordReference>()
        .register_component_as::<dyn Reactive, PublicRecord>()
        .register_component_as::<dyn Reactive, PrivateRecord>();
    let id = Id::new();
    let source = app.world_mut().spawn(RecordReference { record: id }).id();
    let target = app.world_mut().spawn(PublicRecord { value: 0 }).id();
    let mut graph = BindingGraph::new();
    let value = graph.source(BindingPath::new(
        source, "RecordReference", Some("record.PublicRecord.value"),
    ).unwrap()).unwrap();
    graph.bind(value, BindingPath::new(target, "PublicRecord", Some("value")).unwrap()).unwrap();
    graph.install(app.world_mut(), target).unwrap();
    let mut server = app.world().resource::<Session>().get_peer_channel(Id::nil());

    app.update();
    let request = server.try_recv().unwrap().get_ev::<TrackRecordComponentEvent>().unwrap();
    assert_eq!(request.entity_id, id);
    assert_eq!(request.component_type, "PublicRecord");
    let entity = app.world().resource::<DBConfig>().get_entity(&id).unwrap();
    app.update();
    assert!(server.try_recv().is_none());
    assert_eq!(app.world().get::<PublicRecord>(target).unwrap().value, 0);

    app.world_mut().entity_mut(entity).insert(PublicRecord { value: 42 });
    app.update();
    assert_eq!(app.world().get::<PublicRecord>(target).unwrap().value, 42);
    assert!(server.try_recv().is_none());

    let second_target = app.world_mut().spawn(PrivateRecord { value: 0 }).id();
    let mut graph = BindingGraph::new();
    let value = graph.source(BindingPath::new(
        source, "RecordReference", Some("record.PrivateRecord.value"),
    ).unwrap()).unwrap();
    graph.bind(value, BindingPath::new(second_target, "PrivateRecord", Some("value")).unwrap()).unwrap();
    graph.install(app.world_mut(), second_target).unwrap();
    app.add_systems(Update, move |mut commands: FluxCommands| {
        assert_eq!(commands.load_record_component::<PrivateRecord>(id), entity);
        assert_eq!(commands.load_record_component::<PublicRecord>(id), entity);
    });
    app.update();
    let request = server.try_recv().unwrap().get_ev::<TrackRecordComponentEvent>().unwrap();
    assert_eq!(request.entity_id, id);
    assert_eq!(request.component_type, "PrivateRecord");
    assert!(server.try_recv().is_none());
    app.update();
    assert!(server.try_recv().is_none());

    app.world_mut().entity_mut(entity).insert(PrivateRecord { value: 73 });
    app.update();
    assert_eq!(app.world().get::<PrivateRecord>(second_target).unwrap().value, 73);
    assert_eq!(app.world().get::<PublicRecord>(target).unwrap().value, 42);
    assert_eq!(app.world().resource::<DBConfig>().get_entity(&id), Some(entity));
    assert!(server.try_recv().is_none());
}

#[test]
fn record_specific_recipients_require_the_record() {
    let peers = AuthenticatedRecordPeers::default();
    let owner = Id::new();
    let owner_peer = Id::new();
    peers.bind(owner_peer, owner);
    peers.bind(Id::new(), Id::new());
    let policy = RecordPolicy {
        read: RecordReadAccess::Authorized,
        ..RecordPolicy::default()
    };
    assert!(policy.can_read(owner, Some(owner)));
    assert!(peers.readers(policy, owner).is_empty());

    let mut policies = RecordPolicies::default();
    policies.set::<PrivateRecord>(policy);
    let record = PrivateRecord { value: 7 };
    assert!(peers.authorized_readers(&policies, owner, &record).is_empty());
    policies.set_read_rule::<PrivateRecord>(|id, principal, record| {
        id == principal && record.value == 7
    });
    assert_eq!(peers.authorized_readers(&policies, owner, &record), vec![owner_peer]);
    assert!(peers.authorized_readers(&policies, owner, &PrivateRecord { value: 8 }).is_empty());
}

#[cfg(feature = "surrealdb")]
#[test]
fn connected_database_without_preparation_becomes_ready() {
    let mut app = App::new();
    app.add_plugins(bevy::state::app::StatesPlugin)
        .init_state::<DatabaseState>()
        .insert_resource(DBConfig {
            db: std::sync::Arc::new(surrealdb::Surreal::init()),
            id_mappings: Default::default(),
            entity_mappings: Default::default(),
        })
        .add_systems(OnEnter(DatabaseState::Connected), prepare_database);
    app.update();
    assert_eq!(*app.world().resource::<State<DatabaseState>>().get(), DatabaseState::Connecting);
    app.world_mut().resource_mut::<NextState<DatabaseState>>().set(DatabaseState::Connected);
    app.update();
    app.update();
    assert_eq!(*app.world().resource::<State<DatabaseState>>().get(), DatabaseState::Ready);
}

#[cfg(feature = "surrealdb")]
#[test]
fn registration_with_timings_initializes_record_and_preserves_policy() {
    let mut app = App::new();
    app.init_resource::<RecordPolicies>();
    app.world_mut().resource_mut::<RecordPolicies>()
        .set::<PrivateRecord>(RecordPolicy::server_only());
    app.add_record_with_timings::<PrivateRecord>(RecordWriteTiming {
        snapshot_window: std::time::Duration::from_secs(1),
        retry_delay: std::time::Duration::from_secs(2),
        max_starts_per_update: 3,
        max_in_flight: 4,
    }).add_record::<PublicRecord>();

    let timing = app.world().resource::<RecordWriteTimings>().get::<PrivateRecord>();
    assert_eq!(timing.snapshot_window, std::time::Duration::from_secs(1));
    assert_eq!(timing.retry_delay, std::time::Duration::from_secs(2));
    assert_eq!(timing.max_starts_per_update, 3);
    assert_eq!(timing.max_in_flight, 4);
    assert!(app.world().contains_resource::<DBCache<PrivateRecord>>());
    assert!(app.world().contains_resource::<DBCache<PublicRecord>>());
    let policy = app.world().resource::<RecordPolicies>().get::<PrivateRecord>();
    assert_eq!(policy.read, RecordReadAccess::ServerOnly);
    assert!(!policy.automatic_persistence);
}

#[test]
fn loading_a_record_twice_reuses_its_entity() {
    let id = Id::new();
    let mut app = App::new();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    })
    .insert_resource(Session::new(Id::new()))
    .init_resource::<LoadedEntities>()
    .add_event::<PeerEvent>()
    .add_systems(Update, move |mut commands: FluxCommands, mut loaded: ResMut<LoadedEntities>| {
        loaded.0.push(commands.load_record(id));
        loaded.0.push(commands.load_record(id));
    });

    app.update();

    let loaded = &app.world().resource::<LoadedEntities>().0;
    assert_eq!(loaded[0], loaded[1]);
    assert_eq!(app.world().resource::<DBConfig>().get_entity(&id), Some(loaded[0]));
    let mut server = app.world().resource::<Session>().get_peer_channel(Id::nil());
    assert_eq!(server.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, id);
    assert!(server.try_recv().is_none());
}

#[test]
fn component_requests_are_independent_and_reset_with_session_or_entity() {
    let id = Id::new();
    let mut app = App::new();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    })
    .insert_resource(Session::new(Id::new()))
    .add_systems(Update, move |mut commands: FluxCommands| {
        let first = commands.load_record_component::<PublicRecord>(id);
        let second = commands.load_record_component::<PrivateRecord>(id);
        assert_eq!(first, second);
        assert_eq!(first, commands.load_record_component::<PublicRecord>(id));
    });
    let mut server = app.world().resource::<Session>().get_peer_channel(Id::nil());
    let assert_requests = |server: &mut Channel| {
        let mut types = Vec::new();
        while let Some(event) = server.try_recv() {
            let request = event.get_ev::<TrackRecordComponentEvent>().unwrap();
            assert_eq!(request.entity_id, id);
            types.push(request.component_type);
        }
        types.sort();
        assert_eq!(types, vec!["PrivateRecord", "PublicRecord"]);
    };
    app.update();
    assert_requests(&mut server);
    app.update();
    assert!(server.try_recv().is_none());

    let old_entity = app.world().resource::<DBConfig>().get_entity(&id).unwrap();
    app.world_mut().despawn(old_entity);
    app.update();
    assert_requests(&mut server);
    assert_ne!(app.world().resource::<DBConfig>().get_entity(&id), Some(old_entity));

    app.insert_resource(Session::new(Id::new()));
    let mut server = app.world().resource::<Session>().get_peer_channel(Id::nil());
    app.update();
    assert_requests(&mut server);
    app.update();
    assert!(server.try_recv().is_none());
}

#[test]
fn separate_record_types_with_one_id_share_an_entity() {
    let id = Id::new();
    let mut app = App::new();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    })
    .add_systems(Update, move |mut commands: Commands| {
        commands.upsert_record(id, PublicRecord { value: 1 }, |_: InMut<PublicRecord>| {});
        commands.upsert_record(id, PrivateRecord { value: 2 }, |_: InMut<PrivateRecord>| {});
    });

    app.update();

    let entity = app.world().resource::<DBConfig>().get_entity(&id).unwrap();
    assert_eq!(app.world().get::<PublicRecord>(entity).unwrap().value, 1);
    assert_eq!(app.world().get::<PrivateRecord>(entity).unwrap().value, 2);
    assert_eq!(app.world().iter_entities().filter(|entity| entity.get::<DBRecord>().is_some()).count(), 1);
}

#[test]
fn hydrated_components_attach_to_a_loaded_placeholder() {
    let id = Id::new();
    let mut app = App::new();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    })
    .insert_resource(Session::new(Id::new()))
    .add_systems(Update, move |mut commands: FluxCommands| {
        commands.load_record(id);
    });

    app.update();
    let placeholder = app.world().resource::<DBConfig>().get_entity(&id).unwrap();
    app.add_systems(Update, move |mut commands: Commands| {
        commands.upsert_record(id, PublicRecord { value: 1 }, |_: InMut<PublicRecord>| {});
        commands.upsert_record(id, PrivateRecord { value: 2 }, |_: InMut<PrivateRecord>| {});
    });
    app.update();

    assert_eq!(app.world().resource::<DBConfig>().get_entity(&id), Some(placeholder));
    assert_eq!(app.world().get::<PublicRecord>(placeholder).unwrap().value, 1);
    assert_eq!(app.world().get::<PrivateRecord>(placeholder).unwrap().value, 2);
}

#[test]
fn remapping_an_id_or_entity_clears_stale_reverse_entries() {
    let first_id = Id::new();
    let second_id = Id::new();
    let mut world = World::new();
    let first_entity = world.spawn_empty().id();
    let second_entity = world.spawn_empty().id();
    let mut config = DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    };

    config.insert_entity(&first_id, first_entity);
    config.insert_entity(&first_id, second_entity);
    assert!(!config.entity_mappings.contains_key(&first_entity));
    config.insert_entity(&second_id, second_entity);
    assert_eq!(config.get_entity(&first_id), None);
    assert_eq!(config.get_entity(&second_id), Some(second_entity));
}

#[test]
fn loading_after_despawn_replaces_stale_mapping() {
    let id = Id::new();
    let mut app = App::new();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    })
    .insert_resource(Session::new(Id::new()))
    .add_systems(Update, move |mut commands: FluxCommands| {
        commands.load_record(id);
    });

    app.update();
    let first = app.world().resource::<DBConfig>().get_entity(&id).unwrap();
    app.world_mut().despawn(first);
    app.update();

    let config = app.world().resource::<DBConfig>();
    let replacement = config.get_entity(&id).unwrap();
    assert_ne!(first, replacement);
    assert!(!config.entity_mappings.contains_key(&first));
    assert!(app.world().get::<DBRecord>(replacement).is_some());
    let mut server = app.world().resource::<Session>().get_peer_channel(Id::nil());
    assert!(server.try_recv().is_some());
    assert!(server.try_recv().is_some());
    assert!(server.try_recv().is_none());
}

#[test]
fn updating_existing_record_repairs_missing_id_mapping() {
    let id = Id::new();
    let mut app = App::new();
    app.insert_resource(DBConfig {
        #[cfg(feature = "surrealdb")]
        db: std::sync::Arc::new(surrealdb::Surreal::init()),
        id_mappings: Default::default(),
        entity_mappings: Default::default(),
    });
    let entity = app.world_mut().spawn((DBRecord { id }, PublicRecord { value: 0 })).id();
    app.add_systems(Update, move |mut commands: Commands| {
        commands.upsert_record(id, PublicRecord { value: 42 }, |_: InMut<PublicRecord>| {});
    });

    app.update();

    assert_eq!(app.world().get::<PublicRecord>(entity).unwrap().value, 42);
    assert_eq!(app.world().resource::<DBConfig>().get_entity(&id), Some(entity));
}