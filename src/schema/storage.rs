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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaScope {
    pub environment: String,
    pub account_id: uuid::Uuid,
}

impl ReplicaScope {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.account_id.is_nil(), "Replica account must not be nil");
        ensure!(!self.environment.trim().is_empty() && self.environment.len() <= 256,
            "Invalid replica environment");
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingOperation {
    pub format_version: u16,
    pub scope: ReplicaScope,
    pub client_incarnation: uuid::Uuid,
    pub operation_id: uuid::Uuid,
    pub sequence: u64,
    pub operation: String,
    pub request: GraphValue,
}

pub trait OfflineOperation: Schema + Serialize + serde::de::DeserializeOwned {
    const OPERATION: &'static str;
}

impl PendingOperation {
    pub fn encode<Request: OfflineOperation>(
        catalog: &Catalog, scope: ReplicaScope, client_incarnation: uuid::Uuid,
        operation_id: uuid::Uuid, sequence: u64, request: &Request,
    ) -> Result<Self> {
        let request = GraphValue::from_json(&GraphValue::encode_typed(catalog, request)?)?;
        let pending = Self {
            format_version: 1, scope, client_incarnation, operation_id, sequence,
            operation: Request::OPERATION.into(), request,
        };
        pending.validate(&pending.scope)?;
        Ok(pending)
    }

    pub fn validate(&self, scope: &ReplicaScope) -> Result<()> {
        scope.validate()?;
        ensure!(self.scope == *scope, "Operation belongs to another replica scope");
        ensure!(self.format_version == 1, "Unsupported operation format");
        ensure!(!self.client_incarnation.is_nil() && !self.operation_id.is_nil(), "Invalid operation identity");
        ensure!(self.sequence > 0 && self.sequence < i64::MAX as u64, "Invalid operation sequence");
        ensure!(!self.operation.is_empty() && self.operation.len() <= 256
            && self.operation.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
            "Invalid operation name");
        ensure!(serde_json::to_vec(self)?.len() <= MAX_GRAPH_BYTES, "Operation exceeds byte limit");
        Ok(())
    }

    pub fn decode<Request: OfflineOperation>(&self, catalog: &Catalog, scope: &ReplicaScope) -> Result<Request> {
        self.validate(scope)?;
        ensure!(self.operation == Request::OPERATION, "Operation does not match handler");
        GraphValue::decode_typed(catalog, &serde_json::to_vec(&self.request)?)
    }

    pub fn from_json(bytes: &[u8], scope: &ReplicaScope) -> Result<Self> {
        ensure!(bytes.len() <= MAX_GRAPH_BYTES, "Operation exceeds byte limit");
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let value = super::wire::unique_value(&mut decoder)?;
        decoder.end()?;
        let pending: Self = serde_json::from_value(value)?;
        pending.validate(scope)?;
        Ok(pending)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingMutation {
    pub scope: ReplicaScope,
    pub mutation_id: uuid::Uuid,
    pub record_id: uuid::Uuid,
    pub expected_revision: Option<u64>,
    pub payload: GraphValue,
    pub deleted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalReplicaRecord {
    pub scope: ReplicaScope,
    #[serde(default)]
    pub confirmed: Option<StoredRecord>,
    #[serde(default)]
    pub optimistic: Option<String>,
    #[serde(default)]
    pub rejection: Option<MutationRejection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MutationRejection {
    Conflict,
    Forbidden,
    Invalid,
}

impl PendingMutation {
    pub fn validate(&self, catalog: &Catalog, scope: &ReplicaScope) -> Result<()> {
        scope.validate()?;
        ensure!(&self.scope == scope, "Mutation belongs to another replica scope");
        ensure!(!self.mutation_id.is_nil() && !self.record_id.is_nil(), "Invalid mutation identity");
        ensure!(self.expected_revision.is_none_or(|revision| revision > 0 && revision < i64::MAX as u64),
            "Invalid mutation base revision");
        ensure!(!self.deleted || self.expected_revision.is_some(), "Delete requires a confirmed revision");
        ensure!(self.payload.format_version == FORMAT_VERSION, "Unsupported mutation format");
        ensure!(serde_json::to_vec(self)?.len() <= MAX_GRAPH_BYTES, "Mutation exceeds byte limit");
        let (name, root) = catalog.roots.iter().find(|(_, root)|
            root.history.contracts.first().is_some_and(|contract| contract.subject == self.payload.graph.root))
            .ok_or_else(|| anyhow::anyhow!("Unknown mutation subject"))?;
        ensure!(root.database, "Mutation subject is not a database record");
        ensure!(self.payload.graph == catalog.graph_ref(name)?, "Mutation requires the current graph");
        self.payload.graph.validate_value(catalog, &self.payload.value, ValueMode::Complete)
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct SetLabel { label: String }

    impl Schema for SetLabel {
        fn describe() -> DiscoveredContract {
            DiscoveredContract {
                name: "SetLabel".into(), rename_from: None,
                subject: Some("43841eb7-16b4-4659-9eac-034232acb227".into()),
                database: false, wire: true,
                fields: vec![DiscoveredField {
                    name: "label".into(), uuid: None, rename_from: None,
                    ty: ValueType::String, required: true,
                }],
            }
        }
    }

    impl OfflineOperation for SetLabel {
        const OPERATION: &'static str = "test.label.set.v1";
    }

    fn operation_fixture() -> (Catalog, PendingOperation) {
        let catalog = Catalog::init(&[("SetLabel".into(), SetLabel::describe())].into()).unwrap();
        let scope = ReplicaScope { environment: "test-server/database".into(), account_id: uuid::Uuid::new_v4() };
        let operation = PendingOperation::encode(&catalog, scope, uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(), 1, &SetLabel { label: "local edit".into() }).unwrap();
        (catalog, operation)
    }

    #[test]
    fn operation_round_trip_requires_scope_and_handler() {
        let (catalog, operation) = operation_fixture();
        let bytes = serde_json::to_vec(&operation).unwrap();
        let mut restored = PendingOperation::from_json(&bytes, &operation.scope).unwrap();
        assert_eq!(restored.operation_id, operation.operation_id);
        assert_eq!(restored.decode::<SetLabel>(&catalog, &operation.scope).unwrap(),
            SetLabel { label: "local edit".into() });
        let mut other_scope = operation.scope.clone();
        other_scope.account_id = uuid::Uuid::new_v4();
        assert!(PendingOperation::from_json(&bytes, &other_scope).is_err());
        restored.operation = "other.operation.v1".into();
        assert!(restored.decode::<SetLabel>(&catalog, &operation.scope).is_err());
    }

    #[test]
    fn operation_rejects_invalid_identity_and_ambiguous_json() {
        let (_, operation) = operation_fixture();
        let encoded = serde_json::to_string(&operation).unwrap();
        let duplicate = format!("{{\"sequence\":2,{}", &encoded[1..]);
        assert!(PendingOperation::from_json(duplicate.as_bytes(), &operation.scope).is_err());
        assert!(PendingOperation::from_json(format!("{encoded} {{}}").as_bytes(), &operation.scope).is_err());
        let mut invalid = operation.clone();
        invalid.operation_id = uuid::Uuid::nil();
        assert!(invalid.validate(&operation.scope).is_err());
        invalid = operation.clone();
        invalid.client_incarnation = uuid::Uuid::nil();
        assert!(invalid.validate(&operation.scope).is_err());
        invalid = operation.clone();
        invalid.sequence = 0;
        assert!(invalid.validate(&operation.scope).is_err());
        invalid = operation.clone();
        invalid.request.value = serde_json::json!({"label": 42});
        let (catalog, _) = operation_fixture();
        assert!(invalid.decode::<SetLabel>(&catalog, &operation.scope).is_err());
    }

    #[test]
    fn replica_scope_requires_account_and_environment() {
        let mut scope = ReplicaScope { environment: "development".into(), account_id: uuid::Uuid::new_v4() };
        assert!(scope.validate().is_ok());
        scope.account_id = uuid::Uuid::nil();
        assert!(scope.validate().is_err());
        scope.account_id = uuid::Uuid::new_v4();
        scope.environment = " ".into();
        assert!(scope.validate().is_err());
        scope.environment = "x".repeat(257);
        assert!(scope.validate().is_err());
    }
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