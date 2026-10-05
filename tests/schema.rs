#![cfg(feature = "schema")]
use flux::schema::*;
use serde::{Deserialize, Serialize};

#[test]
fn finite_float_schemas_enforce_numeric_ranges() {
    assert_eq!(<f32 as SchemaValueType>::value_type(), ValueType::F32);
    assert_eq!(<f64 as SchemaValueType>::value_type(), ValueType::F64);
    for value in [serde_json::json!(f32::MAX), serde_json::json!(-f32::MAX), serde_json::json!(0.125), serde_json::json!(42)] {
        assert!(ValueType::F32.accepts(&value));
        assert!(ValueType::F64.accepts(&value));
    }
    assert!(!ValueType::F32.accepts(&serde_json::json!(1e39)));
    assert!(ValueType::F64.accepts(&serde_json::json!(f64::MAX)));
    for value in [serde_json::Value::Null, serde_json::json!("1.5"), serde_json::json!(true)] {
        assert!(!ValueType::F32.accepts(&value));
        assert!(!ValueType::F64.accepts(&value));
    }
}

#[cfg(feature = "surrealdb")]
#[test]
fn portable_query_plan_preserves_repeated_fields_and_safe_bindings() {
    use flux::prelude::*;
    let plan = QueryPlan { conditions: vec![
        QueryCondition { field: "count".into(), comparison: QueryComparison::Ge, parameter: "__flux_param_0".into() },
        QueryCondition { field: "count".into(), comparison: QueryComparison::Le, parameter: "__flux_param_1".into() },
    ], limit: Some(3) };
    assert_eq!(compile_surreal_query(&plan, "Counter").unwrap(),
        "SELECT * FROM `Counter` WHERE `count` >= $__flux_param_0 AND `count` <= $__flux_param_1 LIMIT 3");
    assert!(compile_surreal_query(&plan, "Counter; DELETE User").is_err());
    let mut invalid = plan.clone();
    invalid.conditions[0].field = "count` OR true".into();
    assert!(compile_surreal_query(&invalid, "Counter").is_err());
    let mut bindings = QueryBindings::default();
    bindings.insert("__flux_param_0".into(), &1);
    bindings.insert("__flux_param_1".into(), &5);
    let parameters = bindings.into_parameters().unwrap();
    assert_eq!(parameters["__flux_param_0"], serde_json::json!(1));
    assert_eq!(parameters["__flux_param_1"], serde_json::json!(5));
}

#[cfg(feature = "surrealdb")]
#[test]
fn database_plan_lowers_checked_integers_and_enum_mappings() {
    let initial = Contract { format_version: FORMAT_VERSION, subject: uuid::Uuid::new_v4().to_string(), revision: 0,
        fields: [(1, Field { id: 1, name: "count".into(), ty: ValueType::I64, required: true })].into(), reserved_ids: Default::default(), field_uuids: Default::default() };
    let mut target = initial.clone();
    target.revision = 1;
    target.fields.get_mut(&1).unwrap().ty = ValueType::Integer { min: 0, max: 255 };
    let upgrade = Adapter { from_fingerprint: initial.fingerprint().unwrap(), to_fingerprint: target.fingerprint().unwrap(),
        missing: Default::default(), allow_drop: Default::default(), transforms: [(1, Transform::CheckedIntegerV1)].into() };
    let mut history = History::new(initial).unwrap();
    history.append(target, Migration { id: "narrow".into(), from_revision: 0, upgrade, downgrade: None }).unwrap();
    let plan = database::plan("Counter", &history).unwrap();
    assert!(plan.statements[1].contains("$record.`count` <= 255"));
    assert!(!plan.statements[1].contains("SET `count`"));
    let source = ValueType::StringEnum { variants: ["before".into()].into() };
    let target = ValueType::StringEnum { variants: ["after".into()].into() };
    history.contracts[0].fields.get_mut(&1).unwrap().ty = source;
    history.contracts[1].fields.get_mut(&1).unwrap().ty = target;
    history.migrations[0].upgrade.from_fingerprint = history.contracts[0].fingerprint().unwrap();
    history.migrations[0].upgrade.to_fingerprint = history.contracts[1].fingerprint().unwrap();
    history.migrations[0].upgrade.transforms.insert(1, Transform::EnumMapV1 {
        mapping: [("before".into(), "after".into())].into(), allow_lossy: false,
    });
    let plan = database::plan("Counter", &history).unwrap();
    assert!(plan.statements[1].contains("IF $record.`count` = \"before\""));
    assert!(!plan.destructive);
}

#[derive(Schema, Serialize, Deserialize, flux::prelude::Reflect, flux::prelude::Reactive, Clone)]
#[schema(wire, name = "test.managed_event")]
struct ManagedFixture { name: String }

fn managed_fixture_catalog() -> anyhow::Result<Catalog> {
    static CATALOG: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    Ok(CATALOG.get_or_init(|| Catalog::init(&[("test.managed_event".into(), ManagedFixture::describe())].into()).unwrap()).clone())
}

flux::register_managed_event!(ManagedFixture, managed_fixture_catalog);

#[test]
fn managed_network_event_rejects_legacy_and_forged_contracts() {
    use flux::prelude::*;
    let event = NetworkEvent::new(Id::nil(), ManagedFixture { name: "fixture".into() });
    let bytes = serialize_network_event(&event).unwrap();
    let decoded = deserialize_network_event(&bytes).unwrap();
    assert_eq!(decoded.get_ev::<ManagedFixture>().unwrap().name, "fixture");
    assert_eq!(decoded.peer_id, Id::nil());
    assert!(deserialize_network_event(&postcard::to_allocvec(&event).unwrap()).is_err());
    let marker = b"FLUX-GRAPH-1\0";
    let mut envelope: serde_json::Value = serde_json::from_slice(&bytes[marker.len()..]).unwrap();
    let contracts = envelope["payload"]["graph"]["contracts"].as_object_mut().unwrap();
    contracts.values_mut().next().unwrap()["fingerprint"] = "forged".into();
    let mut forged = marker.to_vec();
    forged.extend(serde_json::to_vec(&envelope).unwrap());
    assert!(deserialize_network_event(&forged).is_err());
}

#[derive(Schema, Serialize, Deserialize)]
#[schema(wire, name = "test.recursive")]
struct Recursive {
    value: String,
    next: Option<Box<Recursive>>,
    detail: NestedDetail,
}

#[derive(Schema, Serialize, Deserialize)]
struct NestedDetail { count: u32 }

#[test]
fn database_graph_history_pins_nested_only_revisions() {
    let mut parent = NestedMap::describe();
    parent.database = true;
    let mut roots = std::collections::BTreeMap::from([
        (parent.name.clone(), parent),
        ("NestedDetail".into(), NestedDetail::describe()),
    ]);
    let baseline = Catalog::init(&roots).unwrap();
    let original = &baseline.roots["test.uuid_map"];
    assert_eq!(original.database_graphs, vec![baseline.graph_ref("test.uuid_map").unwrap()]);
    let detail = roots.get_mut("NestedDetail").unwrap();
    detail.fields[0].name = "total".into();
    detail.fields[0].rename_from = Some("count".into());
    let current = baseline.finalize(&baseline.draft("nested_rename".into(), &roots).unwrap()).unwrap();
    let tracked = &current.roots["test.uuid_map"];
    assert_eq!(tracked.history, original.history);
    assert_eq!(tracked.database_graphs.len(), 2);
    assert_eq!(tracked.database_graphs[0], original.database_graphs[0]);
    assert_eq!(tracked.database_graphs[1], current.graph_ref("test.uuid_map").unwrap());
    current.verify_prefix(&baseline).unwrap();
    let restored: Catalog = serde_json::from_slice(&serde_json::to_vec(&current).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, current);
    let mut forged = current.clone();
    forged.roots.get_mut("test.uuid_map").unwrap().database_graphs[0]
        .contracts.values_mut().next().unwrap().fingerprint = "forged".into();
    assert!(forged.validate().is_err());
    let mut reversed = current.clone();
    reversed.roots.get_mut("test.uuid_map").unwrap().database_graphs.reverse();
    assert!(reversed.validate().is_err());
    let mut duplicate = current.clone();
    duplicate.roots.get_mut("test.uuid_map").unwrap().database_graphs.push(tracked.database_graphs[1].clone());
    assert!(duplicate.validate().is_err());
    let mut rewritten = current.clone();
    rewritten.roots.get_mut("test.uuid_map").unwrap().database_graphs.remove(0);
    assert!(rewritten.verify_prefix(&baseline).is_err());
    let mut scalar = NestedDetail::describe();
    scalar.database = true;
    let scalar = Catalog::init(&[(scalar.name.clone(), scalar)].into()).unwrap();
    assert!(!serde_json::to_string(&scalar).unwrap().contains("database_graphs"));
}

#[test]
fn short_name_tracking_can_be_renamed_or_pinned_to_published_id() {
    let mut original = NestedDetail::describe();
    assert_eq!(original.name, "NestedDetail");
    assert_eq!(original.subject, None);
    original.database = true;
    let baseline = Catalog::init(&[(original.name.clone(), original.clone())].into()).unwrap();
    let published = &baseline.roots["NestedDetail"].history;
    let mut renamed = original.clone();
    renamed.name = "RenamedDetail".into();
    assert!(baseline.draft("unannotated".into(), &[(renamed.name.clone(), renamed.clone())].into()).is_err());
    renamed.rename_from = Some("NestedDetail".into());
    let draft = baseline.draft("explicit_rename".into(), &[(renamed.name.clone(), renamed.clone())].into()).unwrap();
    assert!(draft.additions.is_empty() && draft.updates.is_empty());
    let current = baseline.finalize(&draft).unwrap();
    assert_eq!(current.named_root("RenamedDetail"), Some("NestedDetail"));
    assert_eq!(current.roots["NestedDetail"].history, *published);
    current.verify_prefix(&baseline).unwrap();

    original.subject = Some(published.contracts[0].subject.clone());
    assert!(baseline.draft("pin".into(), &[(original.name.clone(), original.clone())].into()).unwrap().is_empty());
    original.name = "PinnedRenamedDetail".into();
    let draft = baseline.draft("id_rename".into(), &[(original.name.clone(), original)].into()).unwrap();
    assert!(draft.additions.is_empty() && draft.updates.is_empty());
    let current = baseline.finalize(&draft).unwrap();
    assert_eq!(current.named_root("PinnedRenamedDetail"), Some("NestedDetail"));
    assert_eq!(current.roots["NestedDetail"].history, *published);
    current.verify_prefix(&baseline).unwrap();
}

#[derive(Schema, Serialize, Deserialize)]
#[schema(wire, name = "test.uuid_map")]
struct NestedMap { entries: std::collections::BTreeMap<uuid::Uuid, NestedDetail> }

#[test]
fn typed_graph_codec_matches_rust_and_rejects_stale_catalogs() {
    let discovered = discovered_roots().unwrap();
    let detail = "NestedDetail";
    let catalog = Catalog::init(&std::collections::BTreeMap::from([
        ("test.recursive".into(), discovered["test.recursive"].clone()),
        (detail.into(), discovered[detail].clone()),
    ])).unwrap();
    let value = Recursive { value: "fixture".into(), next: None, detail: NestedDetail { count: 4 } };
    let bytes = GraphValue::encode_typed(&catalog, &value).unwrap();
    let decoded: Recursive = GraphValue::decode_typed(&catalog, &bytes).unwrap();
    assert_eq!(decoded.detail.count, 4);
    assert_eq!(decoded.value, "fixture");
    let mut envelope = GraphValue::from_json(&bytes).unwrap();
    envelope.value["extra"] = true.into();
    assert!(GraphValue::decode_typed::<Recursive>(&catalog, &serde_json::to_vec(&envelope).unwrap()).is_err());
    let mut stale = catalog.clone();
    stale.roots.get_mut("test.recursive").unwrap().history.contracts[0].fields.get_mut(&1).unwrap().name = "different".into();
    assert!(GraphValue::encode_typed(&stale, &value).is_err());
    assert!(GraphValue::decode_typed::<Recursive>(&stale, &bytes).is_err());
}

#[test]
fn replica_cache_checks_revisions_tombstones_and_reconnect_epochs() {
    let discovered = discovered_roots().unwrap();
    let detail = "NestedDetail";
    let catalog = Catalog::init(&[
        ("test.recursive".into(), discovered["test.recursive"].clone()),
        (detail.into(), discovered[detail].clone()),
    ].into()).unwrap();
    let graph = catalog.graph_ref("test.recursive").unwrap();
    let epoch = uuid::Uuid::new_v4();
    let record_id = uuid::Uuid::new_v4();
    let mut cache = ReplicaCache::new(&catalog, graph.clone(), epoch, 1).unwrap();
    let snapshot = ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision: 10,
        change: ReplicaChange::Snapshot { payload: GraphValue::new(&catalog, "test.recursive",
            serde_json::json!({"value":"kept","next":null,"detail":{"count":1}}), ValueMode::Complete).unwrap() } };
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    cache.apply_authoritative(&catalog, ReplicaEnvelope::from_json(&encoded).unwrap()).unwrap();
    let patch = |revision, base_revision, value| ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision,
        change: ReplicaChange::Patch { base_revision, payload: GraphValue { format_version: FORMAT_VERSION, graph: graph.clone(), value } } };
    assert!(cache.apply_authoritative(&catalog, patch(12, 11, serde_json::json!({"detail":{"count":2}}))).is_err());
    assert!(cache.apply_authoritative(&catalog, patch(11, 10, serde_json::json!({"detail":{"count":-1}}))).is_err());
    assert_eq!(cache.revision(record_id), Some(10));
    assert_eq!(cache.value(record_id).unwrap()["detail"]["count"], 1);
    cache.apply_authoritative(&catalog, patch(11, 10, serde_json::json!({"detail":{"count":2}}))).unwrap();
    assert_eq!(cache.value(record_id).unwrap()["value"], "kept");
    assert_eq!(cache.value(record_id).unwrap()["detail"]["count"], 2);
    let mut over_capacity = snapshot.clone();
    over_capacity.record_id = uuid::Uuid::new_v4();
    assert!(cache.apply_authoritative(&catalog, over_capacity).is_err());
    cache.apply_authoritative(&catalog, ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision: 12,
        change: ReplicaChange::Delete { graph: graph.clone() } }).unwrap();
    assert!(cache.value(record_id).is_none());
    assert_eq!(cache.revision(record_id), Some(12));
    assert!(cache.apply_authoritative(&catalog, snapshot.clone()).is_err());
    assert!(cache.apply_authoritative(&catalog, patch(13, 12, serde_json::json!({"value":"resurrect"}))).is_err());
    assert!(cache.reset(&catalog, graph.clone(), epoch).is_err());
    let next_epoch = uuid::Uuid::new_v4();
    cache.reset(&catalog, graph, next_epoch).unwrap();
    assert!(cache.apply_authoritative(&catalog, snapshot.clone()).is_err());
    let mut refreshed = snapshot;
    refreshed.epoch = next_epoch;
    cache.apply_authoritative(&catalog, refreshed).unwrap();
    assert_eq!(cache.revision(record_id), Some(10));
    let duplicate = String::from_utf8(encoded).unwrap().replacen("\"revision\":10", "\"revision\":10,\"revision\":10", 1);
    assert!(ReplicaEnvelope::from_json(duplicate.as_bytes()).is_err());
}

#[test]
fn graph_collection_patches_validate_complete_entries_and_uuid_keys() {
    assert_eq!(
        <bevy::platform::collections::HashMap<uuid::Uuid, NestedDetail> as SchemaValueType>::value_type(),
        <std::collections::BTreeMap<uuid::Uuid, NestedDetail> as SchemaValueType>::value_type(),
    );
    let discovered = discovered_roots().unwrap();
    let detail_name = "NestedDetail";
    let roots = std::collections::BTreeMap::from([
        ("test.uuid_map".into(), discovered["test.uuid_map"].clone()),
        (detail_name.into(), discovered[detail_name].clone()),
    ]);
    let catalog = Catalog::init(&roots).unwrap();
    let graph = catalog.graph_ref("test.uuid_map").unwrap();
    let id = uuid::Uuid::new_v4();
    let value = serde_json::to_value(NestedMap { entries: [(id, NestedDetail { count: 3 })].into() }).unwrap();
    graph.validate_value(&catalog, &value, ValueMode::Patch).unwrap();
    assert!(graph.validate_value(&catalog, &serde_json::json!({"entries":{id.to_string():{}}}), ValueMode::Patch).is_err());
    assert!(graph.validate_value(&catalog, &serde_json::json!({"entries":{"invalid":{"count":1}}}), ValueMode::Complete).is_err());
    assert!(graph.validate_value(&catalog, &serde_json::json!({"entries":{id.to_string():{"count":1},id.simple().to_string():{"count":2}}}), ValueMode::Complete).is_err());
}

#[test]
fn recursive_discovery_keeps_nested_identities_without_capabilities() {
    let discovered = discovered_roots().unwrap();
    let detail = &discovered["NestedDetail"];
    assert!(!detail.database && !detail.wire);
    let mut roots = std::collections::BTreeMap::from([
        ("test.recursive".into(), discovered["test.recursive"].clone()),
        (detail.name.clone(), detail.clone()),
    ]);
    let catalog = Catalog::init(&roots).unwrap();
    catalog.check(&roots).unwrap();
    let root = &catalog.roots["test.recursive"].history.contracts[0];
    assert_eq!(root.fields[&2].ty, ValueType::Option { value: Box::new(ValueType::Reference { subject: root.subject.clone() }) });
    roots.get_mut(&detail.name).unwrap().subject = Some(catalog.roots[&detail.name].history.contracts[0].subject.clone());
    catalog.check(&roots).unwrap();
}

#[test]
fn graph_adapts_nested_history_and_recursive_patches() {
    let discovered = discovered_roots().unwrap();
    let detail_name = "NestedDetail";
    let mut roots = std::collections::BTreeMap::from([
        ("test.recursive".into(), discovered["test.recursive"].clone()),
        (detail_name.into(), discovered[detail_name].clone()),
    ]);
    let baseline = Catalog::init(&roots).unwrap();
    let original = serde_json::json!({"value":"outer","next":{"value":"inner","next":null,"detail":{"count":2}},"detail":{"count":1}});
    let envelope = GraphValue::new(&baseline, "test.recursive", original, ValueMode::Complete).unwrap();
    let detail = roots.get_mut(detail_name).unwrap();
    detail.fields[0].name = "total".into();
    detail.fields[0].rename_from = Some("count".into());
    detail.fields[0].ty = ValueType::Integer { min: 0, max: 255 };
    detail.fields.push(DiscoveredField { name: "enabled".into(), uuid: None, rename_from: None, ty: ValueType::Bool, required: true });
    let mut draft = baseline.draft("nested".into(), &roots).unwrap();
    draft.updates.get_mut(detail_name).unwrap().migration.upgrade.missing.insert(2, MissingPolicy::Constant { value: serde_json::json!(true) });
    draft.updates.get_mut(detail_name).unwrap().migration.upgrade.transforms.insert(1, Transform::CheckedIntegerV1);
    let current = baseline.finalize(&draft).unwrap();
    let target = current.graph_ref("test.recursive").unwrap();
    let encoded = envelope.to_json(&baseline, ValueMode::Complete).unwrap();
    let restored = GraphValue::from_json(&encoded).unwrap();
    assert_eq!(restored.graph, envelope.graph);
    assert!(GraphValue::from_json(&vec![b' '; MAX_GRAPH_BYTES + 1]).is_err());
    let duplicated = String::from_utf8(encoded).unwrap().replacen("\"format_version\":1", "\"format_version\":1,\"format_version\":1", 1);
    assert!(GraphValue::from_json(duplicated.as_bytes()).is_err());
    assert_eq!(baseline.roots["test.recursive"], current.roots["test.recursive"]);
    assert_ne!(envelope.graph.fingerprint().unwrap(), target.fingerprint().unwrap());
    let decoded = envelope.clone().decode(&current, &target, ValueMode::Complete).unwrap();
    assert_eq!(decoded["detail"], serde_json::json!({"total":1,"enabled":true}));
    assert_eq!(decoded["next"]["detail"], serde_json::json!({"total":2,"enabled":true}));
    let epoch = uuid::Uuid::new_v4();
    let record_id = uuid::Uuid::new_v4();
    let mut replica = ReplicaCache::new(&current, target.clone(), epoch, 1).unwrap();
    replica.apply_authoritative(&current, ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision: 1,
        change: ReplicaChange::Snapshot { payload: envelope.clone() } }).unwrap();
    let historical_patch = GraphValue::new(&baseline, "test.recursive", serde_json::json!({"detail":{"count":4}}), ValueMode::Patch).unwrap();
    replica.apply_authoritative(&current, ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision: 2,
        change: ReplicaChange::Patch { base_revision: 1, payload: historical_patch } }).unwrap();
    assert_eq!(replica.value(record_id).unwrap()["detail"], serde_json::json!({"total":4,"enabled":true}));
    let new_projection = GraphValue::new(&current, "test.recursive", serde_json::json!({"detail":{"enabled":false}}), ValueMode::Patch).unwrap();
    assert!(replica.apply_authoritative(&current, ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision: 3,
        change: ReplicaChange::Patch { base_revision: 2, payload: new_projection } }).is_err());
    assert_eq!(replica.revision(record_id), Some(2));
    let current_record = replica.value(record_id).unwrap().clone();
    let old_write = GraphValue::new(&baseline, "test.recursive", serde_json::json!({"detail":{"count":5}}), ValueMode::Patch).unwrap();
    assert!(target.apply_checked_patch(&current, &current_record, 2, 1, old_write.clone()).is_err());
    let (candidate, revision) = target.apply_checked_patch(&current, &current_record, 2, 2, old_write.clone()).unwrap();
    assert_eq!(revision, 3);
    assert_eq!(candidate["detail"], serde_json::json!({"total":5,"enabled":true}));
    assert_eq!(current_record["detail"]["total"], 4);
    assert!(target.apply_checked_patch(&current, &current_record, u64::MAX, u64::MAX, old_write).is_err());
    assert!(target.adapt(&current, &envelope.graph, decoded.clone(), ValueMode::Complete).is_err());
    let update = draft.updates.get_mut(detail_name).unwrap();
    update.migration.downgrade = Some(Adapter {
        from_fingerprint: update.migration.upgrade.to_fingerprint.clone(),
        to_fingerprint: update.migration.upgrade.from_fingerprint.clone(),
        missing: Default::default(), allow_drop: [2].into(), transforms: [(1, Transform::CheckedIntegerV1)].into(),
    });
    let reversible = baseline.finalize(&draft).unwrap();
    assert_eq!(target.adapt(&reversible, &envelope.graph, decoded, ValueMode::Complete).unwrap(), envelope.value);
    let mut renamed = roots.remove(detail_name).unwrap();
    renamed.name = "test.renamed_detail".into();
    renamed.rename_from = Some(detail_name.into());
    roots.insert(renamed.name.clone(), renamed);
    roots.get_mut("test.recursive").unwrap().fields[2].ty = ValueType::Reference { subject: "test.renamed_detail".into() };
    let renamed = current.finalize(&current.draft("rename_nested_type".into(), &roots).unwrap()).unwrap();
    assert_eq!(renamed.graph_ref("test.recursive").unwrap(), target);
    let patch = GraphValue::new(&baseline, "test.recursive", serde_json::json!({"detail":{"count":3}}), ValueMode::Patch).unwrap();
    assert_eq!(patch.decode(&current, &target, ValueMode::Patch).unwrap(), serde_json::json!({"detail":{"total":3}}));
    let overflow = GraphValue::new(&baseline, "test.recursive", serde_json::json!({"detail":{"count":256}}), ValueMode::Patch).unwrap();
    assert!(overflow.decode(&current, &target, ValueMode::Patch).is_err());
    let mut forged = envelope;
    forged.graph.contracts.values_mut().next().unwrap().fingerprint = "forged".into();
    assert!(forged.decode(&current, &target, ValueMode::Complete).is_err());
    let mut value = serde_json::json!({"value":"leaf","next":null,"detail":{"total":1,"enabled":true}});
    for _ in 0..70 { value = serde_json::json!({"value":"parent","next":value,"detail":{"total":1,"enabled":true}}); }
    assert!(target.validate_value(&current, &value, ValueMode::Complete).is_err());
}

#[derive(Schema, Serialize, Deserialize)]
struct IdentityResponse {
    user_id: Option<flux::prelude::Id>,
}

#[derive(Schema, Serialize, Deserialize)]
enum ScalarState {
    Offline,
    #[serde(rename = "connected")]
    Online,
}

#[derive(Schema, Serialize, Deserialize)]
struct ScalarStateMessage { state: ScalarState }

#[test]
fn scalar_enum_metadata_matches_serde_without_registering_a_root() {
    let contract = ScalarStateMessage::describe().reconcile(None).unwrap();
    for state in [ScalarState::Offline, ScalarState::Online] {
        contract.validate_value(&serde_json::to_value(ScalarStateMessage { state }).unwrap(), ValueMode::Complete).unwrap();
    }
    assert!(contract.validate_value(&serde_json::json!({"state":"Online"}), ValueMode::Complete).is_err());
    assert!(contract.validate_value(&serde_json::json!({"state":{"Offline":null}}), ValueMode::Complete).is_err());
    assert!(!inventory::iter::<Registration>.into_iter().any(|registration| (registration.rust_type)() == std::any::type_name::<ScalarState>()));
}

#[derive(Schema, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum TaggedFailure {
    Unavailable,
    #[serde(rename = "not_allowed")]
    Forbidden,
}

#[test]
fn tagged_enum_schema_matches_serde_and_rejects_unknown_variants() {
    let contract = TaggedFailure::describe().reconcile(None).unwrap();
    for value in [TaggedFailure::Unavailable, TaggedFailure::Forbidden] {
        contract.validate_value(&serde_json::to_value(value).unwrap(), ValueMode::Complete).unwrap();
    }
    assert!(contract.validate_value(&serde_json::json!({"kind":"Unknown"}), ValueMode::Complete).is_err());
    assert!(contract.validate_value(&serde_json::json!({"kind":"Unavailable", "extra":true}), ValueMode::Complete).is_err());
}

#[test]
fn uuid_schema_matches_flux_id_serialization() {
    let contract = IdentityResponse::describe().reconcile(None).unwrap();
    let response = IdentityResponse { user_id: Some(flux::prelude::Id::new()) };
    contract.validate_value(&serde_json::to_value(response).unwrap(), ValueMode::Complete).unwrap();
    contract.validate_value(&serde_json::json!({"user_id": null}), ValueMode::Complete).unwrap();
    assert!(contract.validate_value(&serde_json::json!({"user_id": "invalid"}), ValueMode::Complete).is_err());
}

#[derive(Schema)]
#[schema(database, name = "RenamedStoredProfile", rename_from = "StoredProfile")]
struct RenamedStoredProfile {
    #[schema(id = "693c4a43-d608-4a87-a79a-5629f0a1fa26")]
    name: String,
}

#[cfg(feature = "surrealdb")]
inventory::submit! {
    database::StorageCatalog { json: r#"{
        "format_version":1,
        "roots":{"StoredProfile":{"database":true,"wire":false,"history":{
            "format_version":1,"contracts":[{"format_version":1,
            "subject":"7997d059-e2ab-4db1-995f-2f0699c406d7","revision":0,
            "fields":{},"reserved_ids":[]}],"migrations":[]}}},
        "aliases":{"RenamedStoredProfile":"StoredProfile"}
    }"# }
}

#[test]
fn derive_exposes_type_rename_and_field_uuid() {
    assert_eq!(RenamedStoredProfile { name: "fixture".into() }.name, "fixture");
    let descriptor = RenamedStoredProfile::describe();
    assert_eq!(descriptor.rename_from.as_deref(), Some("StoredProfile"));
    assert_eq!(descriptor.fields[0].uuid.as_deref(), Some("693c4a43-d608-4a87-a79a-5629f0a1fa26"));
    #[cfg(feature = "surrealdb")]
    assert_eq!(database::record_table::<RenamedStoredProfile>("RenamedStoredProfile").unwrap(), "StoredProfile");
}

#[derive(Schema, Serialize, Deserialize, bevy::prelude::Component)]
#[schema(wire, name = "test.profile")]
struct Profile {
    name: String,
    avatar: Option<String>,
}

#[test]
fn replica_projection_replaces_deletes_and_invalidates_ecs_on_reset() {
    use bevy::prelude::World;
    use flux::prelude::{DBRecord, ReplicaProjection};
    let descriptor = Profile::describe();
    let catalog = Catalog::init(&[(descriptor.name.clone(), descriptor)].into()).unwrap();
    let graph = catalog.graph_for::<Profile>().unwrap();
    let epoch = uuid::Uuid::new_v4();
    let record_id = uuid::Uuid::new_v4();
    let mut projection = ReplicaProjection::<Profile>::new(&catalog, graph.clone(), epoch, 4).unwrap();
    let mut world = World::new();
    let snapshot = ReplicaEnvelope { format_version: FORMAT_VERSION, epoch, record_id, revision: 1,
        change: ReplicaChange::Snapshot { payload: GraphValue { format_version: FORMAT_VERSION, graph: graph.clone(),
            value: serde_json::json!({"name":"initial", "avatar":"image"}) } } };
    projection.apply_authoritative(&mut world, &catalog, snapshot.clone()).unwrap();
    let entity = world.query::<(bevy::prelude::Entity, &Profile)>().single(&world).unwrap().0;
    let mut replacement = snapshot.clone();
    replacement.revision = 2;
    if let ReplicaChange::Snapshot { payload } = &mut replacement.change { payload.value = serde_json::json!({"name":"replacement", "avatar":null}); }
    projection.apply_authoritative(&mut world, &catalog, replacement).unwrap();
    assert_eq!(world.get::<Profile>(entity).unwrap().name, "replacement");
    assert!(world.get::<Profile>(entity).unwrap().avatar.is_none());
    let delete = ReplicaEnvelope { revision: 3, change: ReplicaChange::Delete { graph: graph.clone() }, ..snapshot.clone() };
    projection.apply_authoritative(&mut world, &catalog, delete).unwrap();
    assert!(world.get::<Profile>(entity).is_none());
    assert!(world.get::<DBRecord>(entity).is_some());
    assert!(projection.apply_authoritative(&mut world, &catalog, snapshot.clone()).is_err());
    let recreated = ReplicaEnvelope { revision: 4, ..snapshot.clone() };
    projection.apply_authoritative(&mut world, &catalog, recreated).unwrap();
    assert!(projection.reset(&mut world, &catalog, graph.clone(), epoch).is_err());
    assert!(world.get::<Profile>(entity).is_some());
    projection.reset(&mut world, &catalog, graph, uuid::Uuid::new_v4()).unwrap();
    assert!(world.get::<Profile>(entity).is_none());
    assert_eq!(projection.revision(record_id), None);
    assert!(projection.apply_authoritative(&mut world, &catalog, snapshot).is_err());
}

#[derive(Schema, Serialize, Deserialize)]
struct Renamed {
    #[schema(rename_from = "name")]
    display_name: String,
    avatar: Option<String>,
}

#[test]
fn derives_track_only_roots_and_preserve_identity() {
    let roots = discovered_roots().unwrap();
    assert!(roots.contains_key("test.profile"));
    assert!(!roots.contains_key(std::any::type_name::<Renamed>()));
    let original = Profile::describe().reconcile(None).unwrap();
    let renamed = Renamed::describe().reconcile(Some(&original)).unwrap();
    assert_eq!(original.subject, renamed.subject);
    assert_eq!(original.fields[&1].name, "name");
    assert_eq!(renamed.fields[&1].name, "display_name");
    assert_eq!(renamed.revision, 1);
    assert_eq!(Renamed::describe().reconcile(Some(&renamed)).unwrap(), renamed);
}

#[derive(Schema, Serialize, Deserialize)]
struct ScalarCollections {
    #[serde(default, rename = "attempts")]
    count: u8,
    values: std::collections::BTreeMap<String, i16>,
}

#[derive(Default, Schema, Serialize, Deserialize)]
#[serde(default)]
struct DefaultedProfile {
    name: String,
    enabled: bool,
}

#[test]
fn container_defaults_allow_absence_without_inventing_migration_values() {
    let contract = DefaultedProfile::describe().reconcile(None).unwrap();
    assert!(contract.fields.values().all(|field| !field.required));
    contract.validate_value(&serde_json::json!({}), ValueMode::Complete).unwrap();
    let decoded: DefaultedProfile = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(decoded.name, "");
    assert!(!decoded.enabled);
    assert!(contract.validate_value(&serde_json::json!({"enabled":"true"}), ValueMode::Complete).is_err());
}

#[test]
fn derived_integer_ranges_maps_and_combined_defaults_are_checked() {
    let contract = ScalarCollections::describe().reconcile(None).unwrap();
    assert!(!contract.fields[&1].required);
    contract.validate_value(&serde_json::json!({"values": {"low": -32768, "high": 32767}}), ValueMode::Complete).unwrap();
    assert!(contract.validate_value(&serde_json::json!({"attempts": 256, "values": {}}), ValueMode::Complete).is_err());
    assert!(contract.validate_value(&serde_json::json!({"values": {"overflow": 32768}}), ValueMode::Complete).is_err());
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_floats_reject_overflow_and_apply_defaults() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_float_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = DiscoveredContract {
            name: "Usage".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![DiscoveredField { name: "used".into(), uuid: None, rename_from: None, ty: ValueType::F32, required: true }],
        };
        let baseline = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        db.query("CREATE Usage:one SET used = 1e39f;").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &baseline, options).await.is_err());
        assert!(database::verify(&db, &baseline).await.is_err());
        db.query("UPDATE Usage:one SET used = 3.4028235e38f;").await.unwrap().check().unwrap();
        database::apply(&db, &baseline, options).await.unwrap();
        descriptor.fields.push(DiscoveredField { name: "limit".into(), uuid: None, rename_from: None, ty: ValueType::F64, required: true });
        let mut draft = baseline.draft("limit".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        let update = draft.updates.get_mut("Usage").unwrap();
        let field_id = update.target.fields.values().find(|field| field.name == "limit").unwrap().id;
        update.migration.upgrade.missing.insert(field_id, MissingPolicy::Constant { value: serde_json::json!(0.125) });
        let catalog = baseline.finalize(&draft).unwrap();
        database::apply(&db, &catalog, options).await.unwrap();
        database::verify(&db, &catalog).await.unwrap();
        db.query("IF (SELECT VALUE limit FROM ONLY Usage:one) != 0.125f { THROW 'Float default mismatch'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_uuid_maps_reject_aliases_and_apply_object_defaults() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_uuid_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = DiscoveredContract {
            name: "IdentityMap".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![
                DiscoveredField { name: "owner".into(), uuid: None, rename_from: None, ty: ValueType::Uuid, required: true },
                DiscoveredField { name: "values".into(), uuid: None, rename_from: None, ty: ValueType::UuidMap { value: Box::new(ValueType::String) }, required: true },
            ],
        };
        let baseline = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        db.query("CREATE IdentityMap:one SET owner = '550e8400-e29b-41d4-a716-446655440000', values = { invalid: 'bad' };").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &baseline, options).await.is_err());
        db.query("UPDATE IdentityMap:one SET values = object::from_entries([['550e8400-e29b-41d4-a716-446655440000', 'first'], ['550e8400e29b41d4a716446655440000', 'alias']]);").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &baseline, options).await.is_err());
        db.query("UPDATE IdentityMap:one SET values = object::from_entries([['550e8400-e29b-41d4-a716-446655440000', 'valid']]);").await.unwrap().check().unwrap();
        database::apply(&db, &baseline, options).await.unwrap();
        descriptor.fields.push(DiscoveredField { name: "labels".into(), uuid: None, rename_from: None, ty: ValueType::Map { value: Box::new(ValueType::String) }, required: true });
        let mut draft = baseline.draft("labels".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        let update = draft.updates.get_mut("IdentityMap").unwrap();
        let field_id = update.target.fields.values().find(|field| field.name == "labels").unwrap().id;
        update.migration.upgrade.missing.insert(field_id, MissingPolicy::Constant { value: serde_json::json!({"quoted\"key":"value"}) });
        let catalog = baseline.finalize(&draft).unwrap();
        database::apply(&db, &catalog, options).await.unwrap();
        database::verify(&db, &catalog).await.unwrap();
        db.query("IF (SELECT VALUE labels FROM ONLY IdentityMap:one) != object::from_entries([['quoted\"key', 'value']]) { THROW 'Object default mismatch'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_container_transform_validates_nested_values() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_containers_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let container_type = |variant: &str| ValueType::Option { value: Box::new(ValueType::Map { value: Box::new(ValueType::List {
            item: Box::new(ValueType::StringEnum { variants: [variant.to_string()].into() }),
        }) }) };
        let mut descriptor = DiscoveredContract {
            name: "Container".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![DiscoveredField { name: "values".into(), uuid: None, rename_from: None, ty: container_type("old"), required: false }],
        };
        let baseline = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        descriptor.fields[0].ty = container_type("new");
        let mut draft = baseline.draft("map_nested".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        let upgrade = &mut draft.updates.get_mut("Container").unwrap().migration.upgrade;
        upgrade.missing.insert(1, MissingPolicy::PreserveAbsent);
        upgrade.transforms.insert(1, Transform::OptionV1 { value: Box::new(Transform::MapV1 { value: Box::new(Transform::ListV1 {
            item: Box::new(Transform::EnumMapV1 { mapping: [("old".into(), "new".into())].into(), allow_lossy: false }),
        }) }) });
        let catalog = baseline.finalize(&draft).unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        db.query("CREATE Container:valid SET values = { first: ['old', 'old'], empty: [] }; CREATE Container:null SET values = NULL; CREATE Container:absent SET other = true; CREATE Container:bad SET values = { first: ['invalid'] };").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &catalog, options).await.is_err());
        db.query("IF (SELECT VALUE values.first FROM ONLY Container:valid) != ['old', 'old'] { THROW 'Partial transformation'; }; DELETE Container:bad;").await.unwrap().check().unwrap();
        database::apply(&db, &catalog, options).await.unwrap();
        database::verify(&db, &catalog).await.unwrap();
        db.query("IF (SELECT VALUE values.first FROM ONLY Container:valid) != ['new', 'new'] { THROW 'Nested transformation failed'; }; IF (SELECT VALUE values.empty FROM ONLY Container:valid) != [] { THROW 'Empty list changed'; }; IF (SELECT VALUE values FROM ONLY Container:null) != NULL { THROW 'Null changed'; }; IF (SELECT VALUE values FROM ONLY Container:absent) != NONE { THROW 'Absent changed'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_versioned_maintenance_is_atomic_and_preserves_tombstones() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        use flux::schema::storage::VersionedRecordBackend;
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_maintenance_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = Profile::describe();
        descriptor.database = true;
        descriptor.name = "Profile".into();
        let baseline = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        let graph = baseline.graph_ref("Profile").unwrap();
        let backend = database::SurrealRecordBackend(&db);
        let active_id = uuid::Uuid::new_v4();
        let deleted_id = uuid::Uuid::new_v4();
        for record_id in [active_id, deleted_id] {
            backend.compare_exchange(&baseline, record_id, None, GraphValue {
                format_version: FORMAT_VERSION, graph: graph.clone(), value: serde_json::json!({"name":"preserved", "avatar":null}),
            }).await.unwrap();
        }
        backend.delete(&graph.root, deleted_id, 1).await.unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        database::apply(&db, &baseline, options).await.unwrap();
        let field = descriptor.fields.iter_mut().find(|field| field.name == "name").unwrap();
        field.name = "display_name".into();
        field.rename_from = Some("name".into());
        let draft = baseline.draft("rename".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        let next = baseline.finalize(&draft).unwrap();
        db.query("CREATE Profile:conflict SET name = 'before', display_name = 'conflict';").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &next, options).await.is_err());
        database::verify(&db, &baseline).await.unwrap();
        assert_eq!(backend.load(&graph.root, active_id).await.unwrap().unwrap().revision, 1);
        db.query("UPDATE Profile:conflict UNSET display_name;").await.unwrap().check().unwrap();
        database::apply(&db, &next, options).await.unwrap();
        database::verify(&db, &next).await.unwrap();
        for (record_id, revision, deleted) in [(active_id, 2, false), (deleted_id, 3, true)] {
            let stored = backend.load(&graph.root, record_id).await.unwrap().unwrap();
            assert_eq!(stored.revision, revision);
            assert_eq!(stored.deleted, deleted);
            assert_eq!(stored.payload.graph, next.graph_ref("Profile").unwrap());
            assert_eq!(stored.payload.value["display_name"], "preserved");
            assert!(stored.payload.value.get("name").is_none());
        }
        database::apply(&db, &next, options).await.unwrap();
        assert_eq!(backend.load(&graph.root, active_id).await.unwrap().unwrap().revision, 2);
        backend.compare_exchange(&baseline, uuid::Uuid::new_v4(), None, GraphValue {
            format_version: FORMAT_VERSION, graph, value: serde_json::json!({"name":"stale writer", "avatar":null}),
        }).await.unwrap();
        assert!(database::verify(&db, &next).await.is_err());
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_versioned_records_compare_exchange_atomically() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        use flux::schema::storage::VersionedRecordBackend;
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_cas_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = Profile::describe();
        descriptor.database = true;
        let catalog = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        let graph = catalog.graph_ref(&descriptor.name).unwrap();
        let payload = GraphValue { format_version: FORMAT_VERSION, graph: graph.clone(), value: serde_json::json!({"name":"initial", "avatar":null}) };
        let backend = database::SurrealRecordBackend(&db);
        let record_id = uuid::Uuid::new_v4();
        assert!(backend.load(&graph.root, record_id).await.unwrap().is_none());
        let initial = backend.compare_exchange(&catalog, record_id, None, payload.clone()).await.unwrap();
        assert_eq!(initial.revision, 1);
        assert!(backend.compare_exchange(&catalog, record_id, None, payload.clone()).await.is_err());
        let patch = GraphValue { value: serde_json::json!({"name":"updated"}), ..payload.clone() };
        let (value, revision) = graph.apply_checked_patch(&catalog, &initial.payload.value, initial.revision, 1, patch).unwrap();
        assert_eq!(revision, 2);
        let updated = GraphValue { value, ..payload.clone() };
        backend.compare_exchange(&catalog, record_id, Some(1), updated.clone()).await.unwrap();
        assert!(backend.compare_exchange(&catalog, record_id, Some(1), payload.clone()).await.is_err());
        let left = GraphValue { value: serde_json::json!({"name":"left", "avatar":null}), ..payload.clone() };
        let right = GraphValue { value: serde_json::json!({"name":"right", "avatar":null}), ..payload.clone() };
        let (left_result, right_result) = futures::join!(
            backend.compare_exchange(&catalog, record_id, Some(2), left),
            backend.compare_exchange(&catalog, record_id, Some(2), right),
        );
        assert_ne!(left_result.is_ok(), right_result.is_ok());
        let winner = left_result.or(right_result).unwrap();
        let loaded = backend.load(&graph.root, record_id).await.unwrap().unwrap();
        assert_eq!(loaded.revision, 3);
        assert_eq!(loaded.payload.value, winner.payload.value);
        let invalid = GraphValue { value: serde_json::json!({"name":false}), ..payload.clone() };
        assert!(backend.compare_exchange(&catalog, record_id, Some(3), invalid).await.is_err());
        assert_eq!(backend.load(&graph.root, record_id).await.unwrap().unwrap().revision, 3);
        let patch = GraphValue { value: serde_json::json!({"name":"patched"}), ..payload.clone() };
        let patched = flux::schema::storage::patch_record(&backend, &catalog, &graph, record_id, 3, patch.clone()).await.unwrap();
        assert_eq!(patched.revision, 4);
        assert_eq!(patched.payload.value["name"], "patched");
        let epoch = uuid::Uuid::new_v4();
        let mut replica = ReplicaCache::new(&catalog, graph.clone(), epoch, 4).unwrap();
        replica.apply_authoritative(&catalog, patched.replica(epoch, record_id).unwrap()).unwrap();
        assert!(backend.delete(&graph.root, record_id, 3).await.is_err());
        let deleted = backend.delete(&graph.root, record_id, 4).await.unwrap();
        assert_eq!(deleted.revision, 5);
        assert!(deleted.deleted);
        replica.apply_authoritative(&catalog, deleted.replica(epoch, record_id).unwrap()).unwrap();
        assert!(replica.value(record_id).is_none());
        assert_eq!(replica.revision(record_id), Some(5));
        assert!(replica.apply_authoritative(&catalog, patched.replica(epoch, record_id).unwrap()).is_err());
        assert!(backend.load(&graph.root, record_id).await.unwrap().unwrap().deleted);
        assert!(flux::schema::storage::patch_record(&backend, &catalog, &graph, record_id, 5, patch).await.is_err());
        assert!(backend.compare_exchange(&catalog, record_id, None, payload.clone()).await.is_err());
        let recreated = backend.compare_exchange(&catalog, record_id, Some(5), payload).await.unwrap();
        assert_eq!(recreated.revision, 6);
        replica.apply_authoritative(&catalog, recreated.replica(epoch, record_id).unwrap()).unwrap();
        assert!(replica.value(record_id).is_some());
        let mut next_descriptor = descriptor.clone();
        next_descriptor.fields.iter_mut().find(|field| field.name == "name").unwrap().name = "display_name".into();
        next_descriptor.fields.iter_mut().find(|field| field.name == "display_name").unwrap().rename_from = Some("name".into());
        let draft = catalog.draft("rename_profile".into(), &[(descriptor.name.clone(), next_descriptor)].into()).unwrap();
        let next_catalog = catalog.finalize(&draft).unwrap();
        let target = next_catalog.graph_ref(&descriptor.name).unwrap();
        assert!(backend.compare_exchange(&next_catalog, record_id, Some(6), recreated.payload.clone()).await.is_err());
        let old_patch = GraphValue { value: serde_json::json!({"name":"historical"}), ..recreated.payload };
        let migrated = flux::schema::storage::patch_record(&backend, &next_catalog, &target, record_id, 6, old_patch).await.unwrap();
        assert_eq!(migrated.revision, 7);
        assert_eq!(migrated.payload.value["display_name"], "historical");
        assert!(migrated.payload.value.get("name").is_none());
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_enum_transform_requires_loss_approval_and_is_atomic() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        use flux::schema::storage::MigrationBackend;
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_enum_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = DiscoveredContract {
            name: "Status".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![DiscoveredField { name: "state".into(), uuid: None, rename_from: None,
                ty: ValueType::StringEnum { variants: ["old".into(), "older".into()].into() }, required: false }],
        };
        let baseline = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        descriptor.fields[0].name = "status".into();
        descriptor.fields[0].rename_from = Some("state".into());
        descriptor.fields[0].ty = ValueType::StringEnum { variants: ["new".into()].into() };
        let mut draft = baseline.draft("merge_states".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        draft.updates.get_mut("Status").unwrap().migration.upgrade.missing.insert(1, MissingPolicy::PreserveAbsent);
        draft.updates.get_mut("Status").unwrap().migration.upgrade.transforms.insert(1, Transform::EnumMapV1 {
            mapping: [("old".into(), "new".into()), ("older".into(), "new".into())].into(), allow_lossy: true,
        });
        let catalog = baseline.finalize(&draft).unwrap();
        let backend = database::SurrealMigrationBackend(&db);
        assert!(backend.plan(&catalog).unwrap()[0].destructive);
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        db.query("CREATE Status:one SET state = 'old'; CREATE Status:two SET state = 'older', status = 'conflict'; CREATE Status:absent SET unrelated = true;").await.unwrap().check().unwrap();
        backend.apply(&baseline, options, None).await.unwrap();
        assert!(backend.apply(&catalog, options, None).await.is_err());
        let approved = database::ApplyOptions { allow_data_loss: true, ..options };
        assert!(backend.apply(&catalog, approved, None).await.is_err());
        backend.verify(&baseline, None).await.unwrap();
        db.query("IF (SELECT VALUE state FROM ONLY Status:one) != 'old' { THROW 'Partial enum migration'; }; UPDATE Status:two SET status = 'new';").await.unwrap().check().unwrap();
        backend.apply(&catalog, approved, None).await.unwrap();
        backend.verify(&catalog, None).await.unwrap();
        db.query("IF (SELECT VALUE status FROM ONLY Status:one) != 'new' { THROW 'Mapping failed'; }; IF (SELECT VALUE status FROM ONLY Status:two) != 'new' { THROW 'Mapping failed'; }; IF (SELECT VALUE state FROM ONLY Status:one) != NONE { THROW 'Source remains'; }; IF (SELECT VALUE status FROM ONLY Status:absent) != NONE { THROW 'Absent value changed'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
#[cfg(feature = "database_backup")]
fn schema_database_backup_restores_without_overwriting() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let endpoint = std::env::var("FLUX_MIGRATION_TEST_URL").unwrap().replacen("ws://", "http://", 1).replacen("wss://", "https://", 1);
        let db = flux::surrealdb_client::engine::any::connect(&endpoint).await.unwrap();
        let namespace = format!("flux_backup_{}", uuid::Uuid::new_v4().simple());
        db.use_ns(&namespace).use_db("source").await.unwrap();
        db.query("CREATE Counter:one SET count = 42;").await.unwrap().check().unwrap();
        let directory = std::env::temp_dir().join(&namespace);
        std::fs::create_dir(&directory).unwrap();
        let output = directory.join("backup.surql");
        let database = flux::prelude::Database::from_connection(std::sync::Arc::new(db));
        let bytes = database.export_backup(&output).await.unwrap();
        assert_eq!(bytes, std::fs::metadata(&output).unwrap().len());
        let original = std::fs::read(&output).unwrap();
        assert!(database.export_backup(&output).await.is_err());
        assert_eq!(std::fs::read(&output).unwrap(), original);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&output).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let restored = flux::surrealdb_client::engine::any::connect(&endpoint).await.unwrap();
        restored.use_ns(&namespace).use_db("restored").await.unwrap();
        restored.import(&output).await.unwrap();
        let mut response = restored.query("SELECT count FROM Counter;").await.unwrap().check().unwrap();
        let counts: Vec<i64> = response.take((0, "count")).unwrap();
        assert_eq!(counts, vec![42]);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_portable_query_bindings_execute() {
    use flux::prelude::*;
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_query_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        db.query("CREATE Counter:low SET count = 0; CREATE Counter:match SET count = 3; CREATE Counter:high SET count = 8;").await.unwrap().check().unwrap();
        let plan = QueryPlan { conditions: vec![
            QueryCondition { field: "count".into(), comparison: QueryComparison::Ge, parameter: "lower".into() },
            QueryCondition { field: "count".into(), comparison: QueryComparison::Le, parameter: "upper".into() },
        ], limit: None };
        let mut bindings = QueryBindings::default();
        bindings.insert("lower".into(), &1);
        bindings.insert("upper".into(), &5);
        let mut response = db.query(compile_surreal_query(&plan, "Counter").unwrap())
            .bind(flux::surrealdb_client::types::SerdeWrapper(bindings.into_parameters().unwrap()))
            .await.unwrap().check().unwrap();
        let counts: Vec<i64> = response.take((0, "count")).unwrap();
        assert_eq!(counts, vec![3]);
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_scalar_to_nested_preserves_ledger_prefix() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_schema_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut parent = NestedDetail::describe();
        parent.name = "Counter".into();
        parent.database = true;
        let mut roots = std::collections::BTreeMap::from([(parent.name.clone(), parent)]);
        let baseline = Catalog::init(&roots).unwrap();
        roots.insert("NestedDetail".into(), NestedDetail::describe());
        roots.get_mut("Counter").unwrap().fields.push(DiscoveredField { name: "detail".into(), uuid: None, rename_from: None,
            ty: ValueType::Option { value: Box::new(ValueType::Reference { subject: "NestedDetail".into() }) }, required: true });
        let mut draft = baseline.draft("first_nested".into(), &roots).unwrap();
        draft.updates.get_mut("Counter").unwrap().migration.upgrade.missing.insert(2,
            MissingPolicy::Constant { value: serde_json::Value::Null });
        let current = baseline.finalize(&draft).unwrap();
        let before = database::plan_catalog(&baseline).unwrap();
        let after = database::plan_catalog(&current).unwrap();
        assert!(after[0].definitions.starts_with(&before[0].definitions));
        assert_eq!(after[0].statements[0], before[0].statements[0]);
        db.query("CREATE Counter:existing SET count = 1, detail = {count: 9}; CREATE Counter:absent SET count = 2;").await.unwrap().check().unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        database::apply(&db, &baseline, options).await.unwrap();
        database::apply(&db, &current, options).await.unwrap();
        database::verify(&db, &current).await.unwrap();
        db.query("IF (SELECT VALUE detail FROM ONLY Counter:existing) != {count: 9} { THROW 'Existing value overwritten'; }; IF (SELECT VALUE detail FROM ONLY Counter:absent) != NULL { THROW 'Default missing'; }; IF array::len(SELECT * FROM _flux_schema_history) != 2 { THROW 'Scalar history lost'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_nested_graph_migration_is_atomic() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_schema_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut parent = NestedMap::describe();
        parent.name = "NestedRecords".into();
        parent.database = true;
        parent.fields.push(DiscoveredField { name: "batch".into(), uuid: None, rename_from: None,
            ty: ValueType::List { item: Box::new(ValueType::Reference { subject: "NestedDetail".into() }) }, required: true });
        parent.fields.push(DiscoveredField { name: "detail".into(), uuid: None, rename_from: None,
            ty: ValueType::Option { value: Box::new(ValueType::Reference { subject: "NestedDetail".into() }) }, required: false });
        let mut roots = std::collections::BTreeMap::from([
            (parent.name.clone(), parent), ("NestedDetail".into(), NestedDetail::describe()),
        ]);
        let baseline = Catalog::init(&roots).unwrap();
        let detail = roots.get_mut("NestedDetail").unwrap();
        detail.fields[0].name = "total".into();
        detail.fields[0].rename_from = Some("count".into());
        detail.fields[0].ty = ValueType::Integer { min: 0, max: 255 };
        detail.fields.push(DiscoveredField { name: "enabled".into(), uuid: None, rename_from: None, ty: ValueType::Bool, required: true });
        let mut draft = baseline.draft("nested".into(), &roots).unwrap();
        let adapter = &mut draft.updates.get_mut("NestedDetail").unwrap().migration.upgrade;
        adapter.transforms.insert(1, Transform::CheckedIntegerV1);
        adapter.missing.insert(2, MissingPolicy::Constant { value: true.into() });
        let current = baseline.finalize(&draft).unwrap();
        let old_plan = database::plan_catalog(&baseline).unwrap();
        let new_plan = database::plan_catalog(&current).unwrap();
        assert!(new_plan[0].definitions.starts_with(&old_plan[0].definitions));
        db.query("CREATE NestedRecords:one SET entries = {'693c4a43-d608-4a87-a79a-5629f0a1fa26': {count: 4}}, batch = [{count: 5}], detail = {count: 6}, unrelated = 'retained'; CREATE NestedRecords:bad SET entries = {'693c4a43-d608-4a87-a79a-5629f0a1fa26': {count: 256}}, batch = []; CREATE NestedRecords:absent SET entries = {}, batch = []; CREATE NestedRecords:null SET entries = {}, batch = [], detail = NULL;").await.unwrap().check().unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        database::apply(&db, &baseline, options).await.unwrap();
        assert!(database::apply(&db, &current, options).await.is_err());
        db.query("IF array::len(SELECT * FROM _flux_schema_history) != 1 { THROW 'Partial history'; }; IF (SELECT VALUE entries FROM ONLY NestedRecords:one) != {'693c4a43-d608-4a87-a79a-5629f0a1fa26': {count: 4}} { THROW 'Partial migration'; }; DELETE NestedRecords:bad;").await.unwrap().check().unwrap();
        database::apply(&db, &current, options).await.unwrap();
        database::apply(&db, &current, options).await.unwrap();
        database::verify(&db, &current).await.unwrap();
        db.query("IF (SELECT VALUE batch FROM ONLY NestedRecords:one) != [{total: 5, enabled: true}] { THROW 'List migration failed'; }; IF (SELECT VALUE detail FROM ONLY NestedRecords:one) != {total: 6, enabled: true} { THROW 'Option migration failed'; }; IF (SELECT VALUE detail FROM ONLY NestedRecords:absent) != NONE { THROW 'Absent field invented'; }; IF (SELECT VALUE detail FROM ONLY NestedRecords:null) != NULL { THROW 'Null changed'; };").await.unwrap().check().unwrap();
        db.query("IF (SELECT VALUE entries FROM ONLY NestedRecords:one) != {'693c4a43-d608-4a87-a79a-5629f0a1fa26': {total: 4, enabled: true}} { THROW 'Nested migration failed'; }; IF (SELECT VALUE unrelated FROM ONLY NestedRecords:one) != 'retained' { THROW 'Unrelated field lost'; }; IF array::len(SELECT * FROM _flux_schema_history) != 2 { THROW 'Wrong graph history'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_migration_is_atomic_and_detects_drift() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let endpoint = std::env::var("FLUX_MIGRATION_TEST_URL").unwrap();
        let db = flux::surrealdb_client::engine::any::connect(endpoint).await.unwrap();
        db.use_ns(format!("flux_schema_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = Profile::describe();
        descriptor.database = true;
        descriptor.wire = false;
        descriptor.name = "Profile".into();
        let discovered = std::collections::BTreeMap::from([("Profile".into(), descriptor)]);
        let baseline = Catalog::init(&discovered).unwrap();
        let fresh = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        fresh.use_ns(format!("flux_schema_{}", uuid::Uuid::new_v4().simple())).use_db("fresh").await.unwrap();
        database::apply(&fresh, &baseline, database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false }).await.unwrap();
        database::verify(&fresh, &baseline).await.unwrap();
        let mut renamed = Renamed::describe();
        renamed.database = true;
        renamed.name = "Profile".into();
        let next = std::collections::BTreeMap::from([("Profile".into(), renamed)]);
        let draft = baseline.draft("rename".into(), &next).unwrap();
        let catalog = baseline.finalize(&draft).unwrap();
        assert!(database::verify(&db, &catalog).await.is_err());
        db.query("CREATE Profile:one SET name = 'Eden', unrelated = 'retained'; CREATE Profile:two SET name = 'Other', display_name = 'conflict';").await.unwrap().check().unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        let legacy = vec![database::LegacyEntry { migration_id: "legacy_v1".into(), definition: "legacy SQL".into() }];
        assert!(database::apply_with_legacy(&db, &catalog, options, Some(&legacy)).await.is_err());
        db.query("CREATE _flux_migrations:legacy SET position = 0, migration_id = 'legacy_v1', definition = 'legacy SQL';").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &catalog, database::ApplyOptions::default()).await.is_err());
        assert!(database::apply(&db, &catalog, database::ApplyOptions { adopt_baseline: false, ..options }).await.is_err());
        database::apply(&db, &baseline, options).await.unwrap();
        database::verify(&db, &baseline).await.unwrap();
        assert!(database::verify(&db, &catalog).await.is_err());
        assert!(database::apply(&db, &catalog, options).await.is_err());
        db.query("IF (SELECT VALUE name FROM ONLY Profile:one) != 'Eden' { THROW 'Partial migration'; }; IF (SELECT VALUE display_name FROM ONLY Profile:one) != NONE { THROW 'Partial target write'; }; IF array::len(SELECT * FROM _flux_schema_history) != 1 { THROW 'Partial ledger write'; }; DELETE Profile:two;").await.unwrap().check().unwrap();
        database::apply(&db, &catalog, options).await.unwrap();
        database::apply(&db, &catalog, database::ApplyOptions { adopt_baseline: false, ..options }).await.unwrap();
        database::verify(&db, &catalog).await.unwrap();
        database::verify_with_legacy(&db, &catalog, Some(&legacy)).await.unwrap();
        database::apply_with_legacy(&db, &catalog, options, Some(&legacy)).await.unwrap();
        let mut changed_legacy = legacy.clone();
        changed_legacy[0].definition = "changed SQL".into();
        assert!(database::verify_with_legacy(&db, &catalog, Some(&changed_legacy)).await.is_err());
        assert!(database::apply_with_legacy(&db, &catalog, options, Some(&changed_legacy)).await.is_err());
        assert!(database::verify(&db, &baseline).await.is_err());
        db.query("IF (SELECT VALUE display_name FROM ONLY Profile:one) != 'Eden' { THROW 'Rename failed'; }; IF (SELECT VALUE name FROM ONLY Profile:one) != NONE { THROW 'Source remains'; }; IF (SELECT VALUE unrelated FROM ONLY Profile:one) != 'retained' { THROW 'Unrelated field lost'; }; IF array::len(SELECT * FROM _flux_schema_history) != 2 { THROW 'Wrong history count'; };").await.unwrap().check().unwrap();
        assert!(database::apply(&db, &baseline, options).await.is_err());
        let mut drifted = catalog.clone();
        drifted.roots.get_mut("Profile").unwrap().history.migrations[0].id = "changed-rename".into();
        assert!(database::verify(&db, &drifted).await.is_err());
        assert!(database::apply(&db, &drifted, options).await.is_err());
        database::apply(&db, &catalog, options).await.unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_integer_transform_rolls_back_on_overflow() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = flux::surrealdb_client::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns(format!("flux_schema_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = DiscoveredContract {
            name: "Counter".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![DiscoveredField { name: "count".into(), uuid: None, rename_from: None, ty: ValueType::I64, required: true }],
        };
        let baseline = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        descriptor.fields[0].ty = ValueType::Integer { min: 0, max: 255 };
        let mut draft = baseline.draft("narrow_counter".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        draft.updates.get_mut("Counter").unwrap().migration.upgrade.transforms.insert(1, Transform::CheckedIntegerV1);
        let catalog = baseline.finalize(&draft).unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        db.query("CREATE Counter:valid SET count = 255, unrelated = 'retained'; CREATE Counter:overflow SET count = 256;").await.unwrap().check().unwrap();
        database::apply(&db, &baseline, options).await.unwrap();
        assert!(database::apply(&db, &catalog, options).await.is_err());
        database::verify(&db, &baseline).await.unwrap();
        db.query("IF (SELECT VALUE count FROM ONLY Counter:overflow) != 256 { THROW 'Overflow modified'; }; IF array::len(SELECT * FROM _flux_schema_history) != 1 { THROW 'Partial ledger'; }; DELETE Counter:overflow;").await.unwrap().check().unwrap();
        database::apply(&db, &catalog, options).await.unwrap();
        database::verify(&db, &catalog).await.unwrap();
        db.query("IF (SELECT VALUE count FROM ONLY Counter:valid) != 255 { THROW 'Value changed'; }; IF (SELECT VALUE unrelated FROM ONLY Counter:valid) != 'retained' { THROW 'Unmanaged value lost'; }; IF array::len(SELECT * FROM _flux_schema_history) != 2 { THROW 'Missing migration'; };").await.unwrap().check().unwrap();
    });
}

#[cfg(all(feature = "surrealdb", feature = "tokio"))]
#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable server"]
fn schema_database_migration_preserves_snapshots_and_applies_explicit_defaults() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let endpoint = std::env::var("FLUX_MIGRATION_TEST_URL").unwrap();
        let db = flux::surrealdb_client::engine::any::connect(endpoint).await.unwrap();
        db.use_ns(format!("flux_schema_{}", uuid::Uuid::new_v4().simple())).use_db("test").await.unwrap();
        let mut descriptor = DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![
                DiscoveredField { name: "first".into(), uuid: None, rename_from: None, ty: ValueType::String, required: true },
                DiscoveredField { name: "second".into(), uuid: None, rename_from: None, ty: ValueType::String, required: true },
            ],
        };
        let baseline = Catalog::init(&std::collections::BTreeMap::from([("Profile".into(), descriptor.clone())])).unwrap();
        descriptor.fields[0].name = "second".into();
        descriptor.fields[0].rename_from = Some("first".into());
        descriptor.fields[1].name = "first".into();
        descriptor.fields[1].rename_from = Some("second".into());
        descriptor.fields.push(DiscoveredField { name: "enabled".into(), uuid: None, rename_from: None, ty: ValueType::Bool, required: true });
        let mut draft = baseline.draft("swap_and_default".into(), &std::collections::BTreeMap::from([("Profile".into(), descriptor.clone())])).unwrap();
        draft.updates.get_mut("Profile").unwrap().migration.upgrade.missing.insert(3, MissingPolicy::Constant { value: serde_json::json!(true) });
        let catalog = baseline.finalize(&draft).unwrap();
        db.query("CREATE Profile:one SET first = 'A', second = 'B', unrelated = 'retained'; CREATE Profile:invalid SET first = 42, second = 'bad';").await.unwrap().check().unwrap();
        let options = database::ApplyOptions { writers_stopped: true, adopt_baseline: true, allow_data_loss: false };
        assert!(database::apply(&db, &catalog, options).await.is_err());
        db.query("IF (SELECT VALUE first FROM ONLY Profile:one) != 'A' { THROW 'Invalid baseline modified rows'; }; DELETE Profile:invalid;").await.unwrap().check().unwrap();
        database::apply(&db, &catalog, options).await.unwrap();
        db.query("IF (SELECT VALUE first FROM ONLY Profile:one) != 'B' OR (SELECT VALUE second FROM ONLY Profile:one) != 'A' { THROW 'Swap lost snapshot values'; }; IF (SELECT VALUE enabled FROM ONLY Profile:one) != true { THROW 'Missing explicit default'; };").await.unwrap().check().unwrap();
        descriptor.fields.retain(|field| field.name != "enabled");
        for field in &mut descriptor.fields { field.rename_from = None; }
        let mut removal = catalog.draft("remove_enabled".into(), &std::collections::BTreeMap::from([("Profile".into(), descriptor)])).unwrap();
        removal.updates.get_mut("Profile").unwrap().migration.upgrade.allow_drop.insert(3);
        let removed = catalog.finalize(&removal).unwrap();
        assert!(database::apply(&db, &removed, options).await.is_err());
        database::apply(&db, &removed, database::ApplyOptions { allow_data_loss: true, ..options }).await.unwrap();
        db.query("IF (SELECT VALUE enabled FROM ONLY Profile:one) != NONE { THROW 'Field removal failed'; }; IF (SELECT VALUE unrelated FROM ONLY Profile:one) != 'retained' { THROW 'Unmanaged field removed'; }; IF array::len(SELECT * FROM _flux_schema_history) != 3 { THROW 'Wrong history count'; };").await.unwrap().check().unwrap();
    });
}