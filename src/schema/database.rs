use super::*;
use surrealdb::{Surreal, engine::any::Any};
mod graph_plan;
pub use graph_plan::plan_catalog;
pub use super::storage::{ApplyOptions, LegacyEntry};

pub struct SurrealMigrationBackend<'connection>(pub &'connection Surreal<Any>);

pub struct SurrealRecordBackend<'connection>(pub &'connection Surreal<Any>);

impl super::storage::VersionedRecordBackend for SurrealRecordBackend<'_> {
    async fn load(&self, subject: &str, record_id: uuid::Uuid) -> Result<Option<super::storage::StoredRecord>> {
        let subject = uuid::Uuid::parse_str(subject)?;
        let key = format!("{}_{}", subject.simple(), record_id.simple());
        let response = self.0.query("SELECT revision, payload, deleted FROM type::record('_flux_versioned_records', $key)")
            .bind(("key", key)).await?.check();
        let mut response = match response {
            Ok(response) => response,
            Err(error) if matches!(error.not_found_details(), Some(surrealdb::types::NotFoundError::Table { .. })) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let records: Vec<surrealdb::types::SerdeWrapper<super::storage::StoredRecord>> = response.take(0)?;
        Ok(records.into_iter().next().map(|record| record.0))
    }

    async fn compare_exchange(&self, catalog: &Catalog, record_id: uuid::Uuid, expected_revision: Option<u64>, payload: GraphValue) -> Result<super::storage::StoredRecord> {
        ensure!(!record_id.is_nil(), "Versioned record ID must not be nil");
        ensure!(payload.format_version == FORMAT_VERSION, "Unsupported graph format");
        payload.graph.validate_value(catalog, &payload.value, ValueMode::Complete)?;
        ensure!(serde_json::to_vec(&payload)?.len() <= MAX_GRAPH_BYTES, "Stored record exceeds byte limit");
        let (name, root) = catalog.roots.iter().find(|(_, root)| root.history.contracts[0].subject == payload.graph.root)
            .ok_or_else(|| anyhow::anyhow!("Unknown stored record subject"))?;
        ensure!(root.database, "Subject is not a database root");
        ensure!(payload.graph == catalog.graph_ref(name)?, "Writes require the current published graph");
        let revision = expected_revision.unwrap_or(0).checked_add(1).ok_or_else(|| anyhow::anyhow!("Record revision overflow"))?;
        ensure!(revision <= i64::MAX as u64 && expected_revision != Some(0), "Unsupported record revision");
        let subject = uuid::Uuid::parse_str(&payload.graph.root)?;
        let key = format!("{}_{}", subject.simple(), record_id.simple());
        let stored = super::storage::StoredRecord { revision, payload, deleted: false };
        let mut response = self.0.query(
            "BEGIN TRANSACTION;
             DEFINE TABLE IF NOT EXISTS _flux_versioned_records SCHEMALESS PERMISSIONS NONE;
             ALTER TABLE _flux_versioned_records PERMISSIONS NONE;
             LET $current = SELECT * FROM ONLY type::record('_flux_versioned_records', $key);
             IF $creating AND $current != NONE { THROW 'Versioned record already exists'; };
             IF !$creating AND ($current = NONE OR $current.revision != $expected) { THROW 'Record revision conflict'; };
             UPSERT type::record('_flux_versioned_records', $key) CONTENT $record;
             COMMIT TRANSACTION;"
        ).bind(("key", key)).bind(("creating", expected_revision.is_none()))
            .bind(("expected", expected_revision.unwrap_or(0) as i64))
            .bind(("record", surrealdb::types::SerdeWrapper(stored.clone()))).await?;
        ensure!(response.take_errors().is_empty(), "Versioned record transaction failed; record may have changed");
        Ok(stored)
    }

    async fn delete(&self, subject: &str, record_id: uuid::Uuid, expected_revision: u64) -> Result<super::storage::StoredRecord> {
        ensure!(!record_id.is_nil() && expected_revision > 0 && expected_revision < i64::MAX as u64, "Invalid delete revision or identity");
        let subject = uuid::Uuid::parse_str(subject)?;
        let key = format!("{}_{}", subject.simple(), record_id.simple());
        let mut current = <Self as super::storage::VersionedRecordBackend>::load(self, &subject.to_string(), record_id).await?
            .ok_or_else(|| anyhow::anyhow!("Record not found"))?;
        ensure!(!current.deleted && current.revision == expected_revision, "Record revision conflict");
        let mut response = self.0.query(
            "BEGIN TRANSACTION;
             LET $current = SELECT * FROM ONLY type::record('_flux_versioned_records', $key);
             IF $current = NONE OR $current.revision != $expected OR $current.deleted = true { THROW 'Record revision conflict'; };
             UPDATE type::record('_flux_versioned_records', $key) SET revision = $next, deleted = true;
             COMMIT TRANSACTION;"
        ).bind(("key", key)).bind(("expected", expected_revision as i64)).bind(("next", (expected_revision + 1) as i64)).await?;
        ensure!(response.take_errors().is_empty(), "Versioned delete transaction failed; record may have changed");
        current.revision += 1;
        current.deleted = true;
        Ok(current)
    }
}

impl super::storage::MigrationBackend for SurrealMigrationBackend<'_> {
    type Plan = Vec<DatabasePlan>;

    fn plan(&self, catalog: &Catalog) -> Result<Self::Plan> {
        plan_catalog(catalog)
    }

    async fn verify(&self, catalog: &Catalog, legacy: Option<&[LegacyEntry]>) -> Result<()> {
        verify_with_legacy(self.0, catalog, legacy).await
    }

    async fn apply(&self, catalog: &Catalog, options: ApplyOptions, legacy: Option<&[LegacyEntry]>) -> Result<()> {
        apply_with_legacy(self.0, catalog, options, legacy).await
    }
}

pub struct StorageCatalog {
    pub json: &'static str,
}
inventory::collect!(StorageCatalog);

pub fn record_table<T: 'static>(fallback: &str) -> Result<String> {
    static TABLES: std::sync::OnceLock<std::result::Result<BTreeMap<String, String>, String>> = std::sync::OnceLock::new();
    let registration = inventory::iter::<Registration>.into_iter()
        .find(|registration| (registration.rust_type)() == std::any::type_name::<T>());
    let Some(descriptor) = registration.map(|registration| (registration.describe)()) else { return Ok(fallback.into()) };
    if !descriptor.database && descriptor.subject.is_none() { return Ok(fallback.into()); }
    ensure!(descriptor.database, "Schema type is not a database root");
    let subject = descriptor.subject.as_deref().map(uuid::Uuid::parse_str).transpose()?.map(|subject| subject.to_string());
    let tables = TABLES.get_or_init(|| {
        (|| -> Result<BTreeMap<String, String>> {
            let mut tables = BTreeMap::new();
            for source in inventory::iter::<StorageCatalog> {
                let catalog: Catalog = serde_json::from_str(source.json)?;
                catalog.validate()?;
                for (table, root) in &catalog.roots {
                    if !root.database { continue; }
                    identifier(&table)?;
                    let subject = root.history.contracts[0].subject.clone();
                    ensure!(tables.insert(format!("subject:{subject}"), table.clone()).is_none(), "Duplicate database subject mapping");
                    ensure!(tables.insert(format!("name:{table}"), table.clone()).is_none(), "Duplicate database name mapping");
                }
                for (alias, table) in &catalog.aliases {
                    if !catalog.roots[table].database || alias == table { continue; }
                    ensure!(tables.insert(format!("name:{alias}"), table.clone()).is_none(), "Duplicate database alias mapping");
                }
            }
            Ok(tables)
        })().map_err(|error| error.to_string())
    }).as_ref().map_err(|error| anyhow::anyhow!("Invalid storage catalog: {error}"))?;
    let named = tables.get(&format!("name:{}", descriptor.name));
    if let Some(subject) = subject {
        let table = tables.get(&format!("subject:{subject}")).ok_or_else(|| anyhow::anyhow!("No published database table for schema subject {subject}"))?;
        ensure!(named.is_none_or(|named| named == table), "Schema type name and UUID resolve to different database tables");
        Ok(table.clone())
    } else {
        Ok(named.cloned().unwrap_or_else(|| fallback.into()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatabasePlan {
    pub subject: String,
    pub table: String,
    pub definitions: Vec<String>,
    pub statements: Vec<String>,
    pub destructive: bool,
}

fn identifier(name: &str) -> Result<&str> {
    member_identifier(name)?;
    ensure!(name != "id", "The database record id is not a mutable schema field");
    Ok(name)
}

fn member_identifier(name: &str) -> Result<&str> {
    ensure!(name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'), "Unsupported database identifier {name}");
    Ok(name)
}

fn value_literal(value: &Value) -> Result<String> {
    match value {
        Value::Null => Ok("NULL".into()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) if value.as_i64().is_some() => Ok(value.to_string()),
        Value::Number(value) if value.is_f64() && value.as_f64().is_some_and(f64::is_finite) => Ok(format!("{}f", value)),
        Value::String(value) => {
            ensure!(!value.contains('\0'), "NUL is not supported in a database default");
            Ok(serde_json::to_string(value)?)
        }
        Value::Array(values) => Ok(format!("[{}]", values.iter().map(value_literal).collect::<Result<Vec<_>>>()?.join(","))),
        Value::Object(values) => Ok(format!("object::from_entries([{}])", values.iter()
            .map(|(key, value)| Ok(format!("[{},{}]", value_literal(&Value::String(key.clone()))?, value_literal(value)?)))
            .collect::<Result<Vec<_>>>()?.join(","))),
        _ => bail!("Default requires a supported native database value adapter"),
    }
}

fn type_predicate(ty: &ValueType, expression: &str) -> Result<String> {
    type_predicate_at(ty, expression, 0)
}

fn type_predicate_at(ty: &ValueType, expression: &str, depth: usize) -> Result<String> {
    type_predicate_graph(ty, expression, depth, None)
}

fn type_predicate_graph(ty: &ValueType, expression: &str, depth: usize, graph: Option<(&Catalog, &GraphRef)>) -> Result<String> {
    ensure!(depth <= 64, "Database schema exceeds depth limit");
    Ok(match ty {
        ValueType::Bool => format!("type::is_bool({expression})"),
        ValueType::String => format!("type::is_string({expression})"),
        ValueType::Uuid => format!("(IF type::is_uuid({expression}) {{ true }} ELSE IF type::is_string({expression}) {{ string::is_uuid({expression}) }} ELSE {{ false }})"),
        ValueType::StringEnum { variants } => format!("(type::is_string({expression}) AND {expression} IN {})", serde_json::to_string(variants)?),
        ValueType::I64 => format!("type::is_int({expression})"),
        ValueType::U64 => format!("(type::is_int({expression}) AND {expression} >= 0)"),
        ValueType::F32 => format!("(type::is_number({expression}) AND {expression} > -3.4028235677973366e38f AND {expression} < 3.4028235677973366e38f)"),
        ValueType::F64 => format!("(type::is_number({expression}) AND {expression} >= -1.7976931348623157e308f AND {expression} <= 1.7976931348623157e308f)"),
        ValueType::Integer { min, max } => {
            ensure!(*max <= i64::MAX as u64, "Database integer range exceeds native signed integers");
            format!("(type::is_int({expression}) AND {expression} >= {min} AND {expression} <= {max})")
        }
        ValueType::Option { value } => format!("({expression} = NULL OR {})", type_predicate_graph(value, expression, depth + 1, graph)?),
        ValueType::List { item } => {
            let element = format!("$schema_item_{depth}");
            format!("(IF type::is_array({expression}) {{ array::all(array::map({expression}, |{element}| {})) }} ELSE {{ false }})", type_predicate_graph(item, &element, depth + 1, graph)?)
        }
        ValueType::Map { value } => {
            let element = format!("$schema_item_{depth}");
            format!("(IF type::is_object({expression}) {{ array::all(array::map(object::values({expression}), |{element}| {})) }} ELSE {{ false }})", type_predicate_graph(value, &element, depth + 1, graph)?)
        }
        ValueType::UuidMap { value } => {
            let element = format!("$schema_item_{depth}");
            let key = format!("$schema_key_{depth}");
            format!("(IF type::is_object({expression}) {{ IF array::all(array::map(object::keys({expression}), |{key}| string::is_uuid({key}))) {{ array::len(array::distinct(array::map(object::keys({expression}), |{key}| <uuid>{key}))) = array::len(object::keys({expression})) AND array::all(array::map(object::values({expression}), |{element}| {})) }} ELSE {{ false }} }} ELSE {{ false }})", type_predicate_graph(value, &element, depth + 1, graph)?)
        }
        ValueType::Reference { subject } => {
            let (catalog, pinned) = graph.ok_or_else(|| anyhow::anyhow!("Database nested schemas require graph-aware migration; refusing unchecked migration"))?;
            let contract = pinned.contract(catalog, subject)?;
            let mut predicates = Vec::new();
            let names: Vec<_> = contract.fields.values().map(|field| field.name.clone()).collect();
            predicates.push(format!("array::all(array::map(object::keys({expression}), |$schema_field_{depth}| $schema_field_{depth} IN {}))", serde_json::to_string(&names)?));
            for field in contract.fields.values() {
                let field_expression = format!("{expression}.`{}`", member_identifier(&field.name)?);
                let valid = type_predicate_graph(&field.ty, &field_expression, depth + 1, graph)?;
                predicates.push(if field.required { valid } else { format!("({field_expression} = NONE OR {valid})") });
            }
            format!("(IF type::is_object({expression}) {{ {} }} ELSE {{ false }})", predicates.join(" AND "))
        }
    })
}

fn transform_expression(transform: &Transform, source: &str, depth: usize) -> Result<(String, bool)> {
    ensure!(depth <= 64, "Database transform exceeds depth limit");
    Ok(match transform {
        Transform::CheckedIntegerV1 => (source.into(), false),
        Transform::EnumMapV1 { mapping, .. } => {
            let mut expression = source.to_string();
            for (before, after) in mapping.iter().rev() {
                expression = format!("(IF {source} = {} {{ {} }} ELSE {{ {expression} }})",
                    value_literal(&Value::String(before.clone()))?, value_literal(&Value::String(after.clone()))?);
            }
            (expression, mapping.values().collect::<BTreeSet<_>>().len() != mapping.len())
        }
        Transform::OptionV1 { value } => {
            let (expression, lossy) = transform_expression(value, source, depth + 1)?;
            (format!("(IF {source} = NONE OR {source} = NULL {{ {source} }} ELSE {{ {expression} }})"), lossy)
        }
        Transform::ListV1 { item } => {
            let element = format!("$schema_value_{depth}");
            let (expression, lossy) = transform_expression(item, &element, depth + 1)?;
            (format!("(IF {source} = NONE {{ NONE }} ELSE {{ array::map({source}, |{element}| {expression}) }})"), lossy)
        }
        Transform::MapV1 { value } => {
            let entry = format!("$schema_entry_{depth}");
            let (expression, lossy) = transform_expression(value, &format!("{entry}[1]"), depth + 1)?;
            (format!("(IF {source} = NONE {{ NONE }} ELSE {{ object::from_entries(array::map(object::entries({source}), |{entry}| [{entry}[0], {expression}])) }})"), lossy)
        }
        Transform::CustomV1 { .. } => bail!("Database transform requires an explicitly supported native lowering"),
    })
}

fn verify_rows(contract: &Contract, table: &str) -> Result<String> {
    let mut predicates = Vec::new();
    for field in contract.fields.values() {
        let name = identifier(&field.name)?;
        let expression = format!("$record.`{name}`");
        let valid = type_predicate(&field.ty, &expression)?;
        let valid = if field.required { valid } else { format!("({expression} = NONE OR {valid})") };
        predicates.push(format!("IF !({valid}) {{ THROW 'Schema record validation failed'; }};"));
    }
    Ok(format!("LET $schema_rows = SELECT * FROM `{table}`; FOR $record IN $schema_rows {{ {} }};", predicates.join("\n")))
}

pub fn plan(table: &str, history: &History) -> Result<DatabasePlan> {
    history.validate()?;
    let table = identifier(table)?;
    let baseline = &history.contracts[0];
    let baseline_sql = verify_rows(baseline, table)?;
    let mut result = DatabasePlan { subject: baseline.subject.clone(), table: table.into(),
        definitions: vec![format!("surreal-schema-v1:{}:{table}:{baseline_sql}", baseline.fingerprint()?)],
        statements: vec![baseline_sql], destructive: false };
    for (index, migration) in history.migrations.iter().enumerate() {
        let from = &history.contracts[index];
        let to = &history.contracts[index + 1];
        let mut updates = Vec::new();
        let mut cleanup = Vec::new();
        for (id, target) in &to.fields {
            let target_name = identifier(&target.name)?;
            if let Some(source) = from.fields.get(id) {
                let source_name = identifier(&source.name)?;
                let mut expression = format!("$record.`{source_name}`");
                let transform = migration.upgrade.transforms.get(id);
                if let Some(transform) = transform {
                    let (transformed, lossy) = transform_expression(transform, &expression, 0)?;
                    expression = transformed;
                    result.destructive |= lossy;
                }
                if source_name != target_name || transform.is_some_and(|transform| !matches!(transform, Transform::CheckedIntegerV1)) {
                    if source_name != target_name && !from.fields.values().any(|field| field.name == target.name) {
                        updates.push(format!("IF $record.`{target_name}` != NONE AND $record.`{target_name}` != {expression} {{ THROW 'Schema rename conflicts with existing target'; }};"));
                    }
                    updates.push(format!("UPDATE $schema_record_id SET `{target_name}` = {expression};"));
                }
            }
            if let Some(policy) = migration.upgrade.missing.get(id) {
                let source_name = from.fields.get(id).map(|field| field.name.as_str()).unwrap_or(target_name);
                match policy {
                    MissingPolicy::Constant { value } => updates.push(format!("IF $record.`{source_name}` = NONE {{ UPDATE $schema_record_id SET `{target_name}` = {}; }};", value_literal(value)?)),
                    MissingPolicy::PreserveAbsent => {}
                    MissingPolicy::Unsupported => bail!("Database migration {} has an unsupported missing-value policy", migration.id),
                }
            }
        }
        for (id, source) in &from.fields {
            let source_name = identifier(&source.name)?;
            if !to.fields.contains_key(id) { result.destructive = true; }
            if !to.fields.values().any(|field| field.name == source.name) {
                cleanup.push(format!("UPDATE $schema_record_id UNSET `{source_name}`;"));
            }
        }
        let sql = format!("{}\nLET $schema_rows = SELECT * FROM `{table}`; FOR $record IN $schema_rows {{ LET $schema_record_id = $record.id; {} {} }};\n{}",
            verify_rows(from, table)?, updates.join("\n"), cleanup.join("\n"), verify_rows(to, table)?);
        result.definitions.push(format!("surreal-schema-v1:{}:{}:{}:{}", migration.fingerprint()?, from.fingerprint()?, to.fingerprint()?, sql));
        result.statements.push(sql);
    }
    Ok(result)
}

fn legacy_check_sql() -> &'static str {
    "LET $legacy_history = SELECT * FROM _flux_migrations ORDER BY position;
    IF array::len($legacy_history) != array::len($legacy) { THROW 'Legacy migrations must be complete before schema adoption'; };
    FOR $position IN 0..array::len($legacy_history) {
        IF $legacy_history[$position].position != $position
            OR $legacy_history[$position].migration_id != $legacy[$position][0]
            OR $legacy_history[$position].definition != $legacy[$position][1] { THROW 'Legacy migration history drift'; };
    };"
}

fn legacy_values(legacy: Option<&[LegacyEntry]>) -> Vec<Vec<String>> {
    legacy.unwrap_or_default().iter().map(|entry| vec![entry.migration_id.clone(), entry.definition.clone()]).collect()
}

pub async fn verify(db: &Surreal<Any>, catalog: &Catalog) -> Result<()> {
    verify_with_legacy(db, catalog, None).await
}

#[derive(Serialize, Deserialize)]
struct VersionedMaintenanceRow {
    key: String,
    revision: u64,
    payload: GraphValue,
    #[serde(default)]
    deleted: bool,
}

#[derive(Serialize, Deserialize)]
struct VersionedMaintenanceChange {
    expected: VersionedMaintenanceRow,
    replacement: super::storage::StoredRecord,
}

async fn versioned_maintenance(db: &Surreal<Any>, catalog: &Catalog, verify_only: bool) -> Result<Vec<VersionedMaintenanceChange>> {
    let response = db.query("SELECT record::id(id) AS key, revision, payload, deleted FROM _flux_versioned_records LIMIT 4097").await?.check();
    let mut response = match response {
        Ok(response) => response,
        Err(error) if matches!(error.not_found_details(), Some(surrealdb::types::NotFoundError::Table { .. })) => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let rows: Vec<surrealdb::types::SerdeWrapper<VersionedMaintenanceRow>> = response.take(0)?;
    ensure!(rows.len() <= 4096, "Versioned maintenance exceeds 4096 records; bounded batch maintenance is required");
    let mut changes = Vec::new();
    let mut total_bytes = 0usize;
    for row in rows {
        let row = row.0;
        ensure!(row.revision > 0 && row.revision <= i64::MAX as u64, "Invalid stored record revision");
        let subject = uuid::Uuid::parse_str(&row.payload.graph.root)?;
        let prefix = format!("{}_", subject.simple());
        let record_id = row.key.strip_prefix(&prefix).ok_or_else(|| anyhow::anyhow!("Stored record key does not match its subject"))?;
        let record_id = uuid::Uuid::parse_str(record_id)?;
        ensure!(!record_id.is_nil() && row.key == format!("{prefix}{}", record_id.simple()), "Invalid stored record identity");
        let name = catalog.roots.iter().find(|(_, root)| root.database && root.history.contracts[0].subject == row.payload.graph.root)
            .map(|(name, _)| name).ok_or_else(|| anyhow::anyhow!("Stored record is not a published database subject"))?;
        let target = catalog.graph_ref(name)?;
        ensure!(!verify_only || row.payload.graph == target, "Versioned record schema migration required");
        let value = row.payload.clone().decode(catalog, &target, ValueMode::Complete)?;
        let changed = row.payload.graph != target;
        ensure!(!changed || row.revision < i64::MAX as u64, "Record revision exhausted during migration");
        let replacement = super::storage::StoredRecord {
            revision: row.revision + u64::from(changed), deleted: row.deleted,
            payload: GraphValue { format_version: FORMAT_VERSION, graph: target, value },
        };
        let payload_bytes = serde_json::to_vec(&replacement.payload)?.len();
        ensure!(payload_bytes <= MAX_GRAPH_BYTES, "Migrated stored record exceeds byte limit");
        total_bytes += serde_json::to_vec(&row)?.len() + serde_json::to_vec(&replacement)?.len();
        ensure!(total_bytes <= 16 * 1024 * 1024, "Versioned maintenance exceeds 16 MiB; bounded batch maintenance is required");
        changes.push(VersionedMaintenanceChange { expected: row, replacement });
    }
    Ok(changes)
}

pub async fn verify_with_legacy(db: &Surreal<Any>, catalog: &Catalog, legacy: Option<&[LegacyEntry]>) -> Result<()> {
    catalog.validate()?;
    let plans = plan_catalog(catalog)?;
    ensure!(!plans.is_empty(), "No database schema roots");
    let mut sql = String::from("BEGIN TRANSACTION;");
    if legacy.is_some() { sql.push_str(legacy_check_sql()); }
    sql.push_str("
        LET $existing = SELECT VALUE subject FROM _flux_schema_history;
        FOR $subject IN $existing { IF !($subject IN $subjects) { THROW 'Unknown database schema subject'; }; };");
    for (index, _) in plans.iter().enumerate() {
        sql.push_str(&format!("LET $history = SELECT * FROM _flux_schema_history WHERE subject = $subjects[{index}] ORDER BY position;
            IF array::len($history) != array::len($definitions[{index}]) {{ THROW 'Database schema migration required'; }};
            FOR $position IN 0..array::len($history) {{
                IF $history[$position].position != $position OR $history[$position].definition != $definitions[{index}][$position] {{ THROW 'Schema migration history drift'; }};
            }};"));
    }
    sql.push_str("COMMIT TRANSACTION;");
    let subjects: Vec<_> = plans.iter().map(|plan| plan.subject.clone()).collect();
    let definitions: Vec<_> = plans.iter().map(|plan| plan.definitions.clone()).collect();
    let mut response = db.query(sql).bind(("subjects", subjects)).bind(("definitions", definitions))
        .bind(("legacy", legacy_values(legacy))).await?;
    ensure!(response.take_errors().is_empty(), "Database schema is not ready: missing, pending, unknown, or altered migration history");
    versioned_maintenance(db, catalog, true).await?;
    Ok(())
}

pub async fn apply(db: &Surreal<Any>, catalog: &Catalog, options: ApplyOptions) -> Result<()> {
    apply_with_legacy(db, catalog, options, None).await
}

pub async fn apply_with_legacy(db: &Surreal<Any>, catalog: &Catalog, options: ApplyOptions, legacy: Option<&[LegacyEntry]>) -> Result<()> {
    ensure!(options.writers_stopped, "Coordinated maintenance required: stop every writer before applying schema migrations");
    catalog.validate()?;
    let plans = plan_catalog(catalog)?;
    ensure!(!plans.is_empty(), "No database schema roots");
    ensure!(options.allow_data_loss || !plans.iter().any(|plan| plan.destructive), "History contains destructive changes; explicit data-loss approval required");
    let versioned = versioned_maintenance(db, catalog, false).await?;
    let mut sql = String::from("BEGIN TRANSACTION;");
    if legacy.is_some() { sql.push_str(legacy_check_sql()); }
    sql.push_str("
        DEFINE TABLE IF NOT EXISTS _flux_schema_history SCHEMALESS PERMISSIONS NONE;
        ALTER TABLE _flux_schema_history PERMISSIONS NONE;
        DEFINE TABLE IF NOT EXISTS _flux_schema_lock SCHEMALESS PERMISSIONS NONE;
        ALTER TABLE _flux_schema_lock PERMISSIONS NONE;
        UPSERT _flux_schema_lock:head SET touched_at = time::now();
        LET $existing = SELECT VALUE subject FROM _flux_schema_history;
        FOR $subject IN $existing { IF !($subject IN $subjects) { THROW 'Unknown database schema subject'; }; };");
    for (index, plan) in plans.iter().enumerate() {
        sql.push_str(&format!("LET $history = SELECT * FROM _flux_schema_history WHERE subject = $subjects[{index}] ORDER BY position;
            IF array::len($history) = 0 AND !$adopt {{ THROW 'Explicit baseline adoption required'; }};
            IF array::len($history) > array::len($definitions[{index}]) {{ THROW 'Unknown schema migration history'; }};
            FOR $position IN 0..array::len($history) {{
                IF $history[$position].position != $position OR $history[$position].definition != $definitions[{index}][$position] {{ THROW 'Schema migration history drift'; }};
            }};"));
        sql.push_str(&format!("DEFINE TABLE IF NOT EXISTS `{}` SCHEMALESS PERMISSIONS NONE;", plan.table));
        for (position, statement) in plan.statements.iter().enumerate() {
            sql.push_str(&format!("IF array::len($history) <= {position} {{ {statement}
                CREATE _flux_schema_history SET subject = $subjects[{index}], position = {position}, definition = $definitions[{index}][{position}], applied_at = time::now(); }};"));
        }
    }
    sql.push_str("DEFINE TABLE IF NOT EXISTS _flux_versioned_records SCHEMALESS PERMISSIONS NONE;
        ALTER TABLE _flux_versioned_records PERMISSIONS NONE;
        LET $versioned_rows = SELECT VALUE id FROM _flux_versioned_records;
        IF array::len($versioned_rows) != array::len($versioned) { THROW 'Versioned records changed during maintenance'; };
        FOR $change IN $versioned {
            LET $current = SELECT * FROM ONLY type::record('_flux_versioned_records', $change.expected.key);
            IF $current = NONE OR $current.revision != $change.expected.revision
                OR $current.payload != $change.expected.payload
                OR ($current.deleted = true) != $change.expected.deleted { THROW 'Versioned record changed during maintenance'; };
            IF $change.replacement.revision != $change.expected.revision {
                UPDATE type::record('_flux_versioned_records', $change.expected.key) CONTENT $change.replacement;
            };
        };
        COMMIT TRANSACTION;");
    let subjects: Vec<_> = plans.iter().map(|plan| plan.subject.clone()).collect();
    let definitions: Vec<_> = plans.iter().map(|plan| plan.definitions.clone()).collect();
    let mut response = db.query(sql).bind(("subjects", subjects)).bind(("definitions", definitions))
        .bind(("adopt", options.adopt_baseline)).bind(("legacy", legacy_values(legacy)))
        .bind(("versioned", surrealdb::types::SerdeWrapper(versioned))).await?;
    let errors = response.take_errors();
    ensure!(errors.is_empty(), "Schema migration transaction failed at statement indexes {:?}; database error values are withheld", errors.keys().collect::<Vec<_>>());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_plan_rejects_identifier_injection_and_preserves_native_rename() {
        let initial = Contract { format_version: FORMAT_VERSION, subject: "693c4a43-d608-4a87-a79a-5629f0a1fa26".into(), revision: 0,
            fields: BTreeMap::from([(1, Field { id: 1, name: "name".into(), ty: ValueType::String, required: true })]), reserved_ids: BTreeSet::new(), field_uuids: BTreeMap::new() };
        let mut target = initial.clone();
        target.revision = 1;
        target.fields.get_mut(&1).unwrap().name = "display_name".into();
        let upgrade = Adapter { from_fingerprint: initial.fingerprint().unwrap(), to_fingerprint: target.fingerprint().unwrap(), missing: BTreeMap::new(), allow_drop: BTreeSet::new(), transforms: BTreeMap::new() };
        let mut history = History::new(initial).unwrap();
        history.append(target, Migration { id: "rename".into(), from_revision: 0, upgrade, downgrade: None }).unwrap();
        let generated = plan("Profile", &history).unwrap();
        assert!(!generated.destructive);
        assert!(generated.statements[1].contains("SET `display_name` = $record.`name`"));
        assert!(generated.statements[1].contains("Schema rename conflicts"));
        assert!(plan("Profile; DELETE User", &history).is_err());
    }
}