use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredRecord {
    pub revision: u64,
    pub payload: GraphValue,
    #[serde(default)]
    pub deleted: bool,
}

impl StoredRecord {
    pub fn replica(&self, epoch: uuid::Uuid, record_id: uuid::Uuid) -> Result<ReplicaEnvelope> {
        ensure!(!epoch.is_nil() && !record_id.is_nil() && self.revision > 0, "Invalid replica identity");
        let envelope = ReplicaEnvelope {
            format_version: FORMAT_VERSION, epoch, record_id, revision: self.revision,
            change: if self.deleted { ReplicaChange::Delete { graph: self.payload.graph.clone() } }
                else { ReplicaChange::Snapshot { payload: self.payload.clone() } },
        };
        ensure!(serde_json::to_vec(&envelope)?.len() <= MAX_GRAPH_BYTES, "Replica envelope exceeds byte limit");
        Ok(envelope)
    }
}

pub trait VersionedRecordBackend {
    fn load(
        &self, subject: &str, record_id: uuid::Uuid,
    ) -> impl std::future::Future<Output = Result<Option<StoredRecord>>> + Send;

    fn compare_exchange(
        &self, catalog: &Catalog, record_id: uuid::Uuid, expected_revision: Option<u64>, payload: GraphValue,
    ) -> impl std::future::Future<Output = Result<StoredRecord>> + Send;

    fn delete(
        &self, subject: &str, record_id: uuid::Uuid, expected_revision: u64,
    ) -> impl std::future::Future<Output = Result<StoredRecord>> + Send;
}

pub async fn patch_record(
    backend: &impl VersionedRecordBackend, catalog: &Catalog, target: &GraphRef,
    record_id: uuid::Uuid, expected_revision: u64, patch: GraphValue,
) -> Result<StoredRecord> {
    let current = backend.load(&target.root, record_id).await?
        .ok_or_else(|| anyhow::anyhow!("Record not found"))?;
    ensure!(!current.deleted, "Cannot patch a deleted record");
    let value = current.payload.decode(catalog, target, ValueMode::Complete)?;
    let (value, _) = target.apply_checked_patch(catalog, &value, current.revision, expected_revision, patch)?;
    backend.compare_exchange(catalog, record_id, Some(expected_revision), GraphValue {
        format_version: FORMAT_VERSION, graph: target.clone(), value,
    }).await
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ApplyOptions {
    pub writers_stopped: bool,
    pub adopt_baseline: bool,
    pub allow_data_loss: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyEntry {
    pub migration_id: String,
    pub definition: String,
}

/// Executes portable catalog history using a backend-specific physical plan.
///
/// Implementations must reject unsupported operations before writing, verify the
/// immutable ledger prefix, and atomically commit data changes with ledger entries.
/// `writers_stopped` is an operator acknowledgment, not a writer fence. Legacy
/// definitions are backend-specific and must not be transplanted between stores.
/// A backend unable to provide these guarantees must reject application.
pub trait MigrationBackend {
    type Plan;

    fn plan(&self, catalog: &Catalog) -> Result<Self::Plan>;

    fn verify(
        &self, catalog: &Catalog, legacy: Option<&[LegacyEntry]>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;

    fn apply(
        &self, catalog: &Catalog, options: ApplyOptions, legacy: Option<&[LegacyEntry]>,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}