use crate::prelude::*;
use anyhow::{Result, ensure};
use bevy::reflect::TypePath;
use surrealdb::{Surreal, engine::any::Any};

#[derive(Clone, Copy, Debug)]
pub struct RecordFieldMove {
    source_table: &'static str,
    target_table: &'static str,
    field: &'static str,
}

impl RecordFieldMove {
    pub fn new<Source: FluxRecord, Target: FluxRecord>(field: &'static str) -> Self {
        Self {
            source_table: Source::short_type_path(),
            target_table: Target::short_type_path(),
            field,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MigrationStep {
    MoveRecordField(RecordFieldMove),
    MergeRecordField(RecordFieldMove),
}

#[derive(Clone, Copy, Debug)]
pub struct Migration {
    pub id: &'static str,
    pub step: MigrationStep,
}

impl Migration {
    pub fn schema_legacy_entry(&self) -> Result<crate::schema::database::LegacyEntry> {
        checked_identifier(self.id)?;
        let definition = match self.step {
            MigrationStep::MoveRecordField(operation) => field_move_sql(operation)?,
            MigrationStep::MergeRecordField(operation) => field_merge_sql(operation)?,
        };
        Ok(crate::schema::database::LegacyEntry { migration_id: self.id.into(), definition })
    }
}

fn checked_identifier(identifier: &str) -> Result<&str> {
    ensure!(
        identifier.as_bytes().first().is_some_and(|byte| byte.is_ascii_alphabetic())
            && identifier.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "Invalid database identifier: {identifier}"
    );
    Ok(identifier)
}

fn field_move_sql(operation: RecordFieldMove) -> Result<String> {
    let source = checked_identifier(operation.source_table)?;
    let target = checked_identifier(operation.target_table)?;
    let field = checked_identifier(operation.field)?;
    ensure!(source != target, "A field move requires different tables");
    Ok(format!("LET $sources = (SELECT id, `{field}` FROM `{source}` WHERE `{field}` != NONE);
        IF array::len($sources) > 0 {{ FOR $source IN $sources {{
        LET $target = type::record('{target}', record::id($source.id));
        LET $existing = (SELECT * FROM ONLY $target);
        IF $existing != NONE AND $existing.`{field}` != $source.`{field}` {{
            THROW 'Migration target conflicts with source';
        }};
        IF $existing = NONE {{ CREATE $target SET `{field}` = $source.`{field}`; }};
        UPDATE $source.id UNSET `{field}`;
    }}; }};"))
}

fn field_merge_sql(operation: RecordFieldMove) -> Result<String> {
    let source = checked_identifier(operation.source_table)?;
    let target = checked_identifier(operation.target_table)?;
    let field = checked_identifier(operation.field)?;
    ensure!(source != target, "A field move requires different tables");
    Ok(format!("LET $sources = (SELECT id, `{field}` FROM `{source}` WHERE `{field}` != NONE);
        IF array::len($sources) > 0 {{ FOR $source IN $sources {{
        LET $target = type::record('{target}', record::id($source.id));
        LET $existing = (SELECT * FROM ONLY $target);
        IF $existing != NONE AND $existing.`{field}` != NONE AND $existing.`{field}` != $source.`{field}` {{
            THROW 'Migration target conflicts with source';
        }};
        UPSERT $target SET `{field}` = $source.`{field}`;
        UPDATE $source.id UNSET `{field}`;
    }}; }};"))
}

pub async fn run_migrations(migrations: &[Migration]) -> Result<()> {
    let db = get_database().await?;
    run_migrations_on(&db, migrations).await
}

pub async fn run_migrations_on(db: &Surreal<Any>, migrations: &[Migration]) -> Result<()> {
    let mut ids = Vec::new();
    let mut definitions = Vec::new();
    for migration in migrations {
        let id = checked_identifier(migration.id)?;
        ensure!(!ids.contains(&id.to_owned()), "Duplicate migration ID: {id}");
        ids.push(id.to_owned());
        definitions.push(match migration.step {
            MigrationStep::MoveRecordField(operation) => field_move_sql(operation)?,
            MigrationStep::MergeRecordField(operation) => field_merge_sql(operation)?,
        });
    }
    let mut sql = String::from("BEGIN TRANSACTION;
        DEFINE TABLE IF NOT EXISTS _flux_migrations SCHEMALESS PERMISSIONS NONE;
        ALTER TABLE _flux_migrations PERMISSIONS NONE;
        DEFINE TABLE IF NOT EXISTS _flux_migration_state SCHEMALESS PERMISSIONS NONE;
        ALTER TABLE _flux_migration_state PERMISSIONS NONE;
        UPSERT _flux_migration_state:head SET touched_at = time::now();
        LET $history = (SELECT * FROM _flux_migrations ORDER BY position);
        IF array::len($history) > array::len($ids) { THROW 'Unknown migration history'; };
        FOR $position IN 0..array::len($history) {
            LET $entry = $history[$position];
            IF $entry.position != $position OR $entry.migration_id != $ids[$position]
                OR $entry.definition != $definitions[$position] { THROW 'Migration history drift'; };
        };");
    for (position, migration) in migrations.iter().enumerate() {
        sql.push_str(&format!("IF array::len($history) <= {position} {{
            {}
            CREATE _flux_migrations:{} SET migration_id = $ids[{position}],
                position = {position}, definition = $definitions[{position}], applied_at = time::now();
        }};", definitions[position], migration.id));
    }
    sql.push_str("COMMIT TRANSACTION;");
    let mut response = db.query(sql).bind(("ids", ids)).bind(("definitions", definitions)).await?;
    let mut errors: Vec<_> = response.take_errors().into_iter().collect();
    errors.sort_by_key(|(index, _)| *index);
    ensure!(errors.is_empty(), "Migration transaction failed: {}", errors.into_iter()
        .map(|(index, error)| format!("statement {index}: {error}"))
        .collect::<Vec<_>>().join("; "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_restricted_before_query_construction() {
        assert!(checked_identifier("DeviceNetworks_1").is_ok());
        assert!(checked_identifier("Device; DELETE User").is_err());
        assert!(checked_identifier("").is_err());
    }

    #[test]
    fn merging_allows_missing_fields_without_changing_record_move_history() {
        let operation = RecordFieldMove { source_table: "User", target_table: "PrivateUser", field: "email_address" };
        let merge = field_merge_sql(operation).unwrap();
        assert!(merge.contains("$existing.`email_address` != NONE"));
        assert!(merge.contains("UPSERT $target SET `email_address`"));
        assert!(merge.contains("UPDATE $source.id UNSET `email_address`"));
        let original = field_move_sql(operation).unwrap();
        assert!(!original.contains("UPSERT"));
        assert!(!original.contains("$existing.`email_address` != NONE"));
    }
}