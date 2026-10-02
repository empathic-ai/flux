#![cfg(feature = "surrealdb")]

use bevy::prelude::*;
use flux::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Reflect, Reactive, Clone, Debug, Serialize, Deserialize)]
struct LegacyRecord { value: String }

#[derive(Component, Reflect, Reactive, Clone, Debug, Serialize, Deserialize)]
struct SplitRecord { value: String }

#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable database server"]
fn merging_fields_preserves_existing_target_and_is_restart_safe() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let db = surrealdb::engine::any::connect(std::env::var("FLUX_MIGRATION_TEST_URL").unwrap()).await.unwrap();
        db.use_ns("flux_tests").use_db(format!("merge_{}", Id::new().to_pretty_string())).await.unwrap();
        let migrations = [Migration { id: "merge_value_v1", step: MigrationStep::MergeRecordField(
            RecordFieldMove::new::<LegacyRecord, SplitRecord>("value"),
        ) }];
        db.query("CREATE LegacyRecord:one SET value = 'source'; CREATE SplitRecord:one SET unrelated = 'retained'; CREATE LegacyRecord:two SET value = 'source'; CREATE SplitRecord:two SET value = 'conflict';")
            .await.unwrap().check().unwrap();
        assert!(run_migrations_on(&db, &migrations).await.is_err());
        db.query("IF (SELECT VALUE value FROM ONLY LegacyRecord:one) != 'source' { THROW 'Source changed'; }; IF (SELECT VALUE value FROM ONLY SplitRecord:one) != NONE { THROW 'Partial merge'; }; DELETE SplitRecord:two;")
            .await.unwrap().check().unwrap();
        run_migrations_on(&db, &migrations).await.unwrap();
        run_migrations_on(&db, &migrations).await.unwrap();
        db.query("IF (SELECT VALUE unrelated FROM ONLY SplitRecord:one) != 'retained' { THROW 'Unrelated field lost'; }; IF (SELECT VALUE value FROM ONLY SplitRecord:one) != 'source' { THROW 'Value lost'; }; IF (SELECT VALUE value FROM ONLY LegacyRecord:one) != NONE { THROW 'Source not removed'; }; IF array::len(SELECT * FROM _flux_migrations) != 1 { THROW 'Duplicate migration'; };")
            .await.unwrap().check().unwrap();
    });
}

#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable database server"]
fn atomic_record_pair_rolls_back_on_conflict() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let endpoint = std::env::var("FLUX_MIGRATION_TEST_URL").unwrap();
        let connection = surrealdb::engine::any::connect(endpoint).await.unwrap();
        connection.use_ns("flux_tests").use_db(format!("pair_{}", Id::new().to_pretty_string())).await.unwrap();
        let database = Database::from_connection(std::sync::Arc::new(connection));
        let first_id = Id::new();
        let mapping_id = Id::new();
        database.create_record_pair(first_id, LegacyRecord { value: "first".into() },
            mapping_id, SplitRecord { value: "mapping".into() }).await.unwrap();
        let rejected_id = Id::new();
        assert!(database.create_record_pair(rejected_id, LegacyRecord { value: "orphan".into() },
            mapping_id, SplitRecord { value: "replacement".into() }).await.is_err());
        assert!(database.get_record::<LegacyRecord>(rejected_id).await.unwrap().is_none());
        assert_eq!(database.get_record::<LegacyRecord>(first_id).await.unwrap().unwrap().value, "first");
        assert_eq!(database.get_record::<SplitRecord>(mapping_id).await.unwrap().unwrap().value, "mapping");
    });
}

#[test]
#[ignore = "Requires FLUX_MIGRATION_TEST_URL pointing to a disposable database server"]
fn transactional_migration_preserves_values_and_rejects_drift() {
    bevy_wasm_tasks::Runtime::default().block_on(async {
        let endpoint = std::env::var("FLUX_MIGRATION_TEST_URL").unwrap();
        let db = surrealdb::engine::any::connect(endpoint).await.unwrap();
        let database = format!("migration_{}", Id::new().to_pretty_string());
        db.use_ns("flux_tests").use_db(database).await.unwrap();
        let migrations = [Migration {
            id: "split_value_v1",
            step: MigrationStep::MoveRecordField(RecordFieldMove::new::<LegacyRecord, SplitRecord>("value")),
        }];
        db.query("CREATE LegacyRecord:first SET value = { stamp: d'2026-01-01T00:00:00Z', link: User:one }; CREATE LegacyRecord:second SET value = 'source'; CREATE SplitRecord:second SET value = 'conflict';")
            .await.unwrap().check().unwrap();
        db.query("DEFINE TABLE _flux_migrations SCHEMALESS PERMISSIONS NONE;").await.unwrap().check().unwrap();
        let error = run_migrations_on(&db, &migrations).await.unwrap_err();
        assert!(error.to_string().contains("Migration target conflicts with source"), "{error:#}");
        db.query("IF (SELECT VALUE value FROM ONLY LegacyRecord:first) = NONE { THROW 'Lost source'; }; IF (SELECT * FROM ONLY SplitRecord:first) != NONE { THROW 'Partial target'; }; IF array::len(SELECT * FROM _flux_migrations) != 0 { THROW 'Partial ledger'; };")
            .await.unwrap().check().unwrap();
        db.query("DELETE SplitRecord:second;").await.unwrap().check().unwrap();
        run_migrations_on(&db, &migrations).await.unwrap();
        run_migrations_on(&db, &migrations).await.unwrap();
        db.query("IF (SELECT VALUE value FROM ONLY SplitRecord:first) != { stamp: d'2026-01-01T00:00:00Z', link: User:one } { THROW 'Native value changed'; }; IF (SELECT VALUE value FROM ONLY LegacyRecord:first) != NONE { THROW 'Source remains'; }; IF array::len(SELECT * FROM _flux_migrations) != 1 { THROW 'Wrong ledger count'; };")
            .await.unwrap().check().unwrap();
        let error = run_migrations_on(&db, &[]).await.unwrap_err();
        assert!(error.to_string().contains("Unknown migration history"), "{error:#}");
        let changed = [Migration { id: "split_value_v1", step: MigrationStep::MoveRecordField(RecordFieldMove::new::<LegacyRecord, SplitRecord>("other")) }];
        let error = run_migrations_on(&db, &changed).await.unwrap_err();
        assert!(error.to_string().contains("Migration history drift"), "{error:#}");

        db.use_db(format!("migration_{}", Id::new().to_pretty_string())).await.unwrap();
        db.query("DEFINE TABLE LegacyRecord SCHEMALESS; DEFINE TABLE SplitRecord SCHEMALESS;")
            .await.unwrap().check().unwrap();
        let (first, second) = futures::join!(run_migrations_on(&db, &migrations), run_migrations_on(&db, &migrations));
        assert!(first.is_ok() || second.is_ok(), "Both concurrent runners failed: {first:?}, {second:?}");
        run_migrations_on(&db, &migrations).await.unwrap();
        db.query("IF array::len(SELECT * FROM _flux_migrations) != 1 { THROW 'Duplicate history'; };")
            .await.unwrap().check().unwrap();
    });
}