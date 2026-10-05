use super::*;
use super::wire::ContractRef;

const MAX_TYPES: usize = 256;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 100_000;
pub const MAX_GRAPH_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRef {
    pub root: String,
    pub contracts: BTreeMap<String, ContractRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphValue {
    pub format_version: u16,
    pub graph: GraphRef,
    #[serde(deserialize_with = "super::wire::unique_value")]
    pub value: Value,
}

struct Budget { remaining: usize }

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum ReplicaChange {
    Snapshot { payload: GraphValue },
    Patch { base_revision: u64, payload: GraphValue },
    Delete { graph: GraphRef },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaEnvelope {
    pub format_version: u16,
    pub epoch: uuid::Uuid,
    pub record_id: uuid::Uuid,
    pub revision: u64,
    pub change: ReplicaChange,
}

impl ReplicaEnvelope {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_GRAPH_BYTES, "Replica envelope exceeds byte limit");
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let value = super::wire::unique_value(&mut decoder)?;
        decoder.end()?;
        let envelope: Self = serde_json::from_value(value)?;
        ensure!(envelope.format_version == FORMAT_VERSION, "Unsupported replica format");
        Ok(envelope)
    }
}

#[derive(Clone)]
struct ReplicaEntry {
    revision: u64,
    writer_graph: GraphRef,
    value: Option<Value>,
}

#[derive(Clone)]
pub struct ReplicaCache {
    epoch: uuid::Uuid,
    target: GraphRef,
    max_records: usize,
    entries: BTreeMap<uuid::Uuid, ReplicaEntry>,
}

impl ReplicaCache {
    pub fn new(catalog: &Catalog, target: GraphRef, epoch: uuid::Uuid, max_records: usize) -> Result<Self> {
        target.validate(catalog)?;
        ensure!(!epoch.is_nil() && max_records > 0, "Invalid replica session configuration");
        Ok(Self { epoch, target, max_records, entries: BTreeMap::new() })
    }

    pub fn value(&self, record_id: uuid::Uuid) -> Option<&Value> {
        self.entries.get(&record_id).and_then(|entry| entry.value.as_ref())
    }

    pub fn revision(&self, record_id: uuid::Uuid) -> Option<u64> {
        self.entries.get(&record_id).map(|entry| entry.revision)
    }

    pub fn reset(&mut self, catalog: &Catalog, target: GraphRef, epoch: uuid::Uuid) -> Result<()> {
        target.validate(catalog)?;
        ensure!(!epoch.is_nil() && epoch != self.epoch, "Resynchronization requires a fresh epoch");
        self.target = target;
        self.epoch = epoch;
        self.entries.clear();
        Ok(())
    }

    pub fn apply_authoritative(&mut self, catalog: &Catalog, envelope: ReplicaEnvelope) -> Result<()> {
        ensure!(envelope.format_version == FORMAT_VERSION && envelope.epoch == self.epoch, "Replica session mismatch; resynchronize");
        ensure!(serde_json::to_vec(&envelope)?.len() <= MAX_GRAPH_BYTES, "Replica envelope exceeds byte limit");
        let previous = self.entries.get(&envelope.record_id);
        ensure!(previous.is_some() || self.entries.len() < self.max_records, "Replica cache capacity exceeded; resynchronize");
        ensure!(previous.is_none_or(|entry| envelope.revision > entry.revision), "Stale replica revision");
        let (writer_graph, value) = match envelope.change {
            ReplicaChange::Snapshot { payload } => {
                let graph = payload.graph.clone();
                let value = payload.decode(catalog, &self.target, ValueMode::Complete)?;
                ensure!(serde_json::to_vec(&value)?.len() <= MAX_GRAPH_BYTES, "Replica record exceeds byte limit");
                (graph, Some(value))
            }
            ReplicaChange::Patch { base_revision, payload } => {
                let previous = previous.ok_or_else(|| anyhow::anyhow!("Replica patch needs a snapshot"))?;
                ensure!(previous.revision == base_revision && base_revision.checked_add(1) == Some(envelope.revision), "Replica revision gap; resynchronize");
                ensure!(previous.writer_graph == payload.graph, "Writer projection changed; resynchronize");
                let mut value = previous.value.clone().ok_or_else(|| anyhow::anyhow!("Cannot patch a deleted record"))?;
                let graph = payload.graph.clone();
                let patch = payload.decode(catalog, &self.target, ValueMode::Patch)?;
                self.target.merge_patch(catalog, &self.target.root, &mut value, patch)?;
                self.target.validate_value(catalog, &value, ValueMode::Complete)?;
                ensure!(serde_json::to_vec(&value)?.len() <= MAX_GRAPH_BYTES, "Replica record exceeds byte limit");
                (graph, Some(value))
            }
            ReplicaChange::Delete { graph } => {
                graph.validate(catalog)?;
                ensure!(graph.root == self.target.root, "Replica delete subject mismatch");
                (graph, None)
            }
        };
        self.entries.insert(envelope.record_id, ReplicaEntry { revision: envelope.revision, writer_graph, value });
        Ok(())
    }
}

impl Budget {
    fn new() -> Self { Self { remaining: MAX_NODES } }

    fn visit(&mut self, depth: usize) -> Result<()> {
        ensure!(depth <= MAX_DEPTH, "Schema value exceeds depth limit");
        self.remaining = self.remaining.checked_sub(1).ok_or_else(|| anyhow::anyhow!("Schema value exceeds node limit"))?;
        Ok(())
    }
}

impl Catalog {
    pub fn graph_for<T: Schema>(&self) -> Result<GraphRef> {
        self.validate()?;
        let descriptor = T::describe();
        let key = self.named_root(&descriptor.name).ok_or_else(|| anyhow::anyhow!("Unpublished schema type {}", descriptor.name))?;
        let root = &self.roots[key];
        ensure!(root.wire, "Type is not a published wire root");
        let registered: BTreeMap<_, _> = inventory::iter::<Registration>.into_iter().map(|registration| {
            let descriptor = (registration.describe)();
            (descriptor.name.clone(), descriptor)
        }).collect();
        let mut discovered = BTreeMap::new();
        let mut pending = BTreeSet::from([descriptor.name.clone()]);
        while let Some(name) = pending.pop_first() {
            if discovered.contains_key(&name) { continue; }
            ensure!(discovered.len() < MAX_TYPES, "Schema graph exceeds type limit");
            let current = if name == descriptor.name { &descriptor } else {
                registered.get(&name).ok_or_else(|| anyhow::anyhow!("Unregistered nested schema {name}"))?
            };
            for field in &current.fields { field.ty.references(&mut pending); }
            discovered.insert(name, current.clone());
        }
        for (key, descriptor) in self.resolve_descriptors(&discovered)? {
            let published = self.roots.get(&key).ok_or_else(|| anyhow::anyhow!("Unpublished nested schema {key}"))?;
            let latest = published.history.contracts.last().unwrap();
            ensure!(descriptor.reconcile(Some(latest))? == *latest, "Rust graph type differs from published contract {key}");
        }
        self.graph_ref(&descriptor.name)
    }

    pub(crate) fn subject_history(&self, subject: &str) -> Result<&History> {
        self.roots.values().find(|tracked| tracked.history.contracts[0].subject == subject)
            .map(|tracked| &tracked.history).ok_or_else(|| anyhow::anyhow!("Unknown graph subject {subject}"))
    }

    pub fn graph_ref(&self, name: &str) -> Result<GraphRef> {
        self.validate()?;
        let key = self.named_root(name).ok_or_else(|| anyhow::anyhow!("Unknown graph root {name}"))?;
        let root = self.roots[key].history.contracts[0].subject.clone();
        let mut contracts = BTreeMap::new();
        let mut pending = BTreeSet::from([root.clone()]);
        while let Some(subject) = pending.pop_first() {
            if contracts.contains_key(&subject) { continue; }
            ensure!(contracts.len() < MAX_TYPES, "Schema graph exceeds type limit");
            let contract = self.subject_history(&subject)?.contracts.last().unwrap();
            for field in contract.fields.values() { field.ty.references(&mut pending); }
            contracts.insert(subject, ContractRef::new(contract)?);
        }
        Ok(GraphRef { root, contracts })
    }
}

impl GraphRef {
    pub fn apply_checked_patch(&self, catalog: &Catalog, current: &Value, current_revision: u64, expected_revision: u64, patch: GraphValue) -> Result<(Value, u64)> {
        ensure!(current_revision == expected_revision, "Record revision conflict");
        let next_revision = current_revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("Record revision exhausted"))?;
        ensure!(serde_json::to_vec(&patch)?.len() <= MAX_GRAPH_BYTES, "Record patch exceeds byte limit");
        self.validate_value(catalog, current, ValueMode::Complete)?;
        let patch = patch.decode(catalog, self, ValueMode::Patch)?;
        let mut candidate = current.clone();
        self.merge_patch(catalog, &self.root, &mut candidate, patch)?;
        self.validate_value(catalog, &candidate, ValueMode::Complete)?;
        ensure!(serde_json::to_vec(&candidate)?.len() <= MAX_GRAPH_BYTES, "Patched record exceeds byte limit");
        Ok((candidate, next_revision))
    }

    fn merge_patch(&self, catalog: &Catalog, subject: &str, value: &mut Value, patch: Value) -> Result<()> {
        let contract = self.contract(catalog, subject)?;
        let object = value.as_object_mut().ok_or_else(|| anyhow::anyhow!("Expected replica object"))?;
        let patch = patch.as_object().ok_or_else(|| anyhow::anyhow!("Expected replica patch"))?;
        for field in contract.fields.values() {
            let Some(update) = patch.get(&field.name) else { continue };
            let mut ty = &field.ty;
            while let ValueType::Option { value } = ty { ty = value; }
            if let ValueType::Reference { subject } = ty {
                if !update.is_null() {
                    let current = object.entry(field.name.clone()).or_insert_with(|| Value::Object(Map::new()));
                    if current.is_null() { *current = Value::Object(Map::new()); }
                    self.merge_patch(catalog, subject, current, update.clone())?;
                    continue;
                }
            }
            object.insert(field.name.clone(), update.clone());
        }
        Ok(())
    }

    pub(crate) fn contract<'catalog>(&self, catalog: &'catalog Catalog, subject: &str) -> Result<&'catalog Contract> {
        self.contracts.get(subject).ok_or_else(|| anyhow::anyhow!("Missing graph contract {subject}"))?
            .validate(catalog.subject_history(subject)?)
    }

    pub fn validate(&self, catalog: &Catalog) -> Result<()> {
        catalog.validate()?;
        self.validate_contracts(catalog)
    }

    pub(crate) fn validate_contracts(&self, catalog: &Catalog) -> Result<()> {
        ensure!(self.contracts.len() <= MAX_TYPES, "Schema graph exceeds type limit");
        let mut pending = BTreeSet::from([self.root.clone()]);
        let mut visited = BTreeSet::new();
        while let Some(subject) = pending.pop_first() {
            if !visited.insert(subject.clone()) { continue; }
            let contract = self.contract(catalog, &subject)?;
            ensure!(contract.subject == subject, "Graph key and subject disagree");
            for field in contract.fields.values() { field.ty.references(&mut pending); }
        }
        ensure!(visited.len() == self.contracts.len(), "Unreachable contracts in schema graph");
        Ok(())
    }

    pub fn fingerprint(&self) -> Result<String> {
        Ok(format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn validate_value(&self, catalog: &Catalog, value: &Value, mode: ValueMode) -> Result<()> {
        self.validate(catalog)?;
        self.validate_subject(catalog, &self.root, value, mode, &mut Budget::new(), 0)
    }

    fn validate_subject(&self, catalog: &Catalog, subject: &str, value: &Value, mode: ValueMode, budget: &mut Budget, depth: usize) -> Result<()> {
        budget.visit(depth)?;
        let contract = self.contract(catalog, subject)?;
        let object = value.as_object().ok_or_else(|| anyhow::anyhow!("Expected graph object"))?;
        ensure!(object.len() <= contract.fields.len() && object.keys().all(|name| contract.fields.values().any(|field| &field.name == name)), "Unknown graph fields");
        for field in contract.fields.values() {
            if let Some(value) = object.get(&field.name) {
                self.validate_type(catalog, &field.ty, value, mode, budget, depth + 1)?;
            } else {
                ensure!(mode == ValueMode::Patch || !field.required, "Missing graph field {}", field.name);
            }
        }
        Ok(())
    }

    fn validate_type(&self, catalog: &Catalog, ty: &ValueType, value: &Value, mode: ValueMode, budget: &mut Budget, depth: usize) -> Result<()> {
        budget.visit(depth)?;
        match ty {
            ValueType::Reference { subject } => self.validate_subject(catalog, subject, value, mode, budget, depth + 1),
            ValueType::Option { value: inner } if !value.is_null() => self.validate_type(catalog, inner, value, mode, budget, depth + 1),
            ValueType::List { item } => {
                for value in value.as_array().ok_or_else(|| anyhow::anyhow!("Expected graph list"))? {
                    self.validate_type(catalog, item, value, ValueMode::Complete, budget, depth + 1)?;
                }
                Ok(())
            }
            ValueType::Map { value: item } | ValueType::UuidMap { value: item } => {
                let items = value.as_object().ok_or_else(|| anyhow::anyhow!("Expected graph map"))?;
                ensure!(!matches!(ty, ValueType::UuidMap { .. }) || uuid_keys_valid(items), "Invalid or duplicate UUID map key");
                for value in items.values() {
                    self.validate_type(catalog, item, value, ValueMode::Complete, budget, depth + 1)?;
                }
                Ok(())
            }
            _ => { ensure!(ty.accepts(value), "Invalid graph scalar"); Ok(()) }
        }
    }

    pub fn adapt(&self, catalog: &Catalog, target: &GraphRef, value: Value, mode: ValueMode) -> Result<Value> {
        ensure!(self.root == target.root, "Cannot adapt different graph roots");
        self.validate_value(catalog, &value, mode)?;
        target.validate(catalog)?;
        let value = self.adapt_subject(catalog, target, &self.root, value, mode, &mut Budget::new(), 0)?;
        target.validate_value(catalog, &value, mode)?;
        Ok(value)
    }

    fn adapt_subject(&self, catalog: &Catalog, target: &GraphRef, subject: &str, mut value: Value, mode: ValueMode, budget: &mut Budget, depth: usize) -> Result<Value> {
        budget.visit(depth)?;
        let history = catalog.subject_history(subject)?;
        let from = self.contract(catalog, subject)?.revision as usize;
        let to = target.contract(catalog, subject)?.revision as usize;
        if from < to {
            for index in from..to {
                value = history.migrations[index].upgrade.project(&history.contracts[index], &history.contracts[index + 1], value, mode)?;
            }
        } else {
            for index in (to..from).rev() {
                value = history.migrations[index].downgrade.as_ref().ok_or_else(|| anyhow::anyhow!("Graph downgrade unavailable"))?
                    .project(&history.contracts[index + 1], &history.contracts[index], value, mode)?;
            }
        }
        let object = value.as_object_mut().ok_or_else(|| anyhow::anyhow!("Expected adapted graph object"))?;
        for field in history.contracts[to].fields.values() {
            if let Some(value) = object.remove(&field.name) {
                object.insert(field.name.clone(), self.adapt_type(catalog, target, &field.ty, value, mode, budget, depth + 1)?);
            }
        }
        Ok(value)
    }

    fn adapt_type(&self, catalog: &Catalog, target: &GraphRef, ty: &ValueType, value: Value, mode: ValueMode, budget: &mut Budget, depth: usize) -> Result<Value> {
        budget.visit(depth)?;
        match (ty, value) {
            (ValueType::Reference { subject }, value) => self.adapt_subject(catalog, target, subject, value, mode, budget, depth + 1),
            (ValueType::Option { value: inner }, value) if !value.is_null() => self.adapt_type(catalog, target, inner, value, mode, budget, depth + 1),
            (ValueType::List { item }, Value::Array(items)) => Ok(Value::Array(items.into_iter().map(|value| self.adapt_type(catalog, target, item, value, ValueMode::Complete, budget, depth + 1)).collect::<Result<_>>()?)),
            (ValueType::Map { value: item } | ValueType::UuidMap { value: item }, Value::Object(items)) => Ok(Value::Object(items.into_iter().map(|(key, value)| Ok((key, self.adapt_type(catalog, target, item, value, ValueMode::Complete, budget, depth + 1)?))).collect::<Result<_>>()?)),
            (_, value) => Ok(value),
        }
    }
}

impl GraphValue {
    pub fn encode_typed<T: Schema + Serialize>(catalog: &Catalog, value: &T) -> Result<Vec<u8>> {
        let graph = catalog.graph_for::<T>()?;
        let value = serde_json::to_value(value)?;
        Self { format_version: FORMAT_VERSION, graph, value }.to_json(catalog, ValueMode::Complete)
    }

    pub fn decode_typed<T: Schema + serde::de::DeserializeOwned>(catalog: &Catalog, bytes: &[u8]) -> Result<T> {
        let graph = catalog.graph_for::<T>()?;
        let value = Self::from_json(bytes)?.decode(catalog, &graph, ValueMode::Complete)?;
        Ok(serde_json::from_value(value)?)
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_GRAPH_BYTES, "Schema graph payload exceeds byte limit");
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let value = super::wire::unique_value(&mut decoder)?;
        decoder.end()?;
        let envelope: Self = serde_json::from_value(value)?;
        ensure!(envelope.format_version == FORMAT_VERSION, "Unsupported schema graph format");
        ensure!(envelope.graph.contracts.len() <= MAX_TYPES, "Schema graph exceeds type limit");
        Ok(envelope)
    }

    pub fn to_json(&self, catalog: &Catalog, mode: ValueMode) -> Result<Vec<u8>> {
        ensure!(self.format_version == FORMAT_VERSION, "Unsupported schema graph format");
        self.graph.validate_value(catalog, &self.value, mode)?;
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= MAX_GRAPH_BYTES, "Schema graph payload exceeds byte limit");
        Ok(bytes)
    }

    pub fn new(catalog: &Catalog, root: &str, value: Value, mode: ValueMode) -> Result<Self> {
        let graph = catalog.graph_ref(root)?;
        graph.validate_value(catalog, &value, mode)?;
        Ok(Self { format_version: FORMAT_VERSION, graph, value })
    }

    pub fn decode(self, catalog: &Catalog, target: &GraphRef, mode: ValueMode) -> Result<Value> {
        ensure!(self.format_version == FORMAT_VERSION, "Unsupported schema graph format");
        self.graph.adapt(catalog, target, self.value, mode)
    }
}