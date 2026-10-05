use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAT_VERSION: u16 = 1;

mod history;
pub use history::*;
mod discovery;
pub use discovery::*;
mod catalog;
pub use catalog::*;
pub use flux_derive::Schema;
pub use inventory;
pub mod wire;
mod graph;
pub use graph::*;
mod transform;
pub use transform::*;
pub mod storage;
#[cfg(feature = "surrealdb")]
pub mod database;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ValueType {
    Bool,
    String,
    Uuid,
    StringEnum { variants: BTreeSet<String> },
    I64,
    U64,
    F32,
    F64,
    Integer { min: i64, max: u64 },
    Option { value: Box<ValueType> },
    List { item: Box<ValueType> },
    Map { value: Box<ValueType> },
    UuidMap { value: Box<ValueType> },
    Reference { subject: String },
}

impl ValueType {
    pub fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::Bool => value.is_boolean(),
            Self::String => value.is_string(),
            Self::Uuid => value.as_str().is_some_and(|value| uuid::Uuid::parse_str(value).is_ok()),
            Self::StringEnum { variants } => value.as_str().is_some_and(|value| variants.contains(value)),
            Self::I64 => value.as_i64().is_some(),
            Self::U64 => value.as_u64().is_some(),
            Self::F32 => value.as_f64().is_some_and(|value| value.is_finite() && (value as f32).is_finite()),
            Self::F64 => value.as_f64().is_some_and(f64::is_finite),
            Self::Integer { min, max } => value.as_i64().is_some_and(|value| value < 0 && value >= *min)
                || value.as_u64().is_some_and(|value| value <= *max && (*min <= 0 || value >= *min as u64)),
            Self::Option { value: inner } => value.is_null() || inner.accepts(value),
            Self::List { item } => value.as_array().is_some_and(|items| items.iter().all(|value| item.accepts(value))),
            Self::Map { value: item } => value.as_object().is_some_and(|items| items.values().all(|value| item.accepts(value))),
            Self::UuidMap { value: item } => value.as_object().is_some_and(|items| uuid_keys_valid(items) && items.values().all(|value| item.accepts(value))),
            Self::Reference { .. } => false,
        }
    }

    pub(crate) fn references(&self, output: &mut BTreeSet<String>) {
        match self {
            Self::Reference { subject } => { output.insert(subject.clone()); }
            Self::Option { value } | Self::Map { value } | Self::UuidMap { value } => value.references(output),
            Self::List { item } => item.references(output),
            _ => {}
        }
    }

    fn resolve(&mut self, subjects: &BTreeMap<String, String>) -> Result<()> {
        match self {
            Self::Reference { subject } => {
                *subject = subjects.get(subject).cloned().ok_or_else(|| anyhow::anyhow!("Unregistered nested schema {subject}"))?;
            }
            Self::Option { value } | Self::Map { value } | Self::UuidMap { value } => value.resolve(subjects)?,
            Self::List { item } => item.resolve(subjects)?,
            _ => {}
        }
        Ok(())
    }
}

fn uuid_keys_valid(items: &Map<String, Value>) -> bool {
    let mut keys = BTreeSet::new();
    items.keys().all(|key| uuid::Uuid::parse_str(key).is_ok_and(|key| keys.insert(key)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: u32,
    pub name: String,
    pub ty: ValueType,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub format_version: u16,
    pub subject: String,
    pub revision: u32,
    pub fields: BTreeMap<u32, Field>,
    pub reserved_ids: BTreeSet<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub field_uuids: BTreeMap<String, u32>,
}

impl Contract {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.format_version == FORMAT_VERSION, "Unsupported contract format");
        ensure!(uuid::Uuid::parse_str(&self.subject).is_ok(), "Subject must be a UUID");
        let mut names = BTreeSet::new();
        let mut uuid_fields = BTreeSet::new();
        for (uuid, id) in &self.field_uuids {
            ensure!(uuid::Uuid::parse_str(uuid)?.to_string() == *uuid, "Field UUID must be canonical");
            ensure!(self.fields.contains_key(id) || self.reserved_ids.contains(id), "Unknown UUID field identity");
            ensure!(uuid_fields.insert(*id), "Multiple UUIDs assigned to one field");
        }
        for (id, field) in &self.fields {
            ensure!(*id != 0 && *id == field.id, "Invalid field identity {id}");
            ensure!(!self.reserved_ids.contains(id), "Reserved field identity {id}");
            ensure!(!field.name.is_empty() && names.insert(&field.name), "Duplicate or empty field name");
            let mut references = BTreeSet::new();
            field.ty.references(&mut references);
            for subject in references { ensure!(uuid::Uuid::parse_str(&subject)?.to_string() == subject, "Nested subject must be a canonical UUID"); }
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn validate_value(&self, value: &Value, mode: ValueMode) -> Result<()> {
        self.validate()?;
        let object = value.as_object().ok_or_else(|| anyhow::anyhow!("Expected object"))?;
        ensure!(object.len() <= self.fields.len(), "Unknown fields in payload");
        for name in object.keys() {
            ensure!(self.fields.values().any(|field| &field.name == name), "Unknown field {name}");
        }
        for field in self.fields.values() {
            match object.get(&field.name) {
                Some(value) => ensure!(field.ty.accepts(value), "Invalid value for {}", field.name),
                None => ensure!(mode == ValueMode::Patch || !field.required, "Missing field {}", field.name),
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueMode {
    Complete,
    Patch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum MissingPolicy {
    PreserveAbsent,
    Constant { value: Value },
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    pub from_fingerprint: String,
    pub to_fingerprint: String,
    pub missing: BTreeMap<u32, MissingPolicy>,
    pub allow_drop: BTreeSet<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transforms: BTreeMap<u32, Transform>,
}

impl Adapter {
    pub fn validate(&self, from: &Contract, to: &Contract) -> Result<()> {
        ensure!(from.subject == to.subject, "Cannot implicitly adapt different subjects");
        ensure!(self.from_fingerprint == from.fingerprint()?, "Source contract mismatch");
        ensure!(self.to_fingerprint == to.fingerprint()?, "Target contract mismatch");
        for id in &self.allow_drop {
            ensure!(from.fields.contains_key(id) && !to.fields.contains_key(id), "Invalid drop decision {id}");
        }
        for (id, transform) in &self.transforms {
            let source = from.fields.get(id).ok_or_else(|| anyhow::anyhow!("Transform has no source field {id}"))?;
            let target = to.fields.get(id).ok_or_else(|| anyhow::anyhow!("Transform has no target field {id}"))?;
            transform.validate(&source.ty, &target.ty)?;
        }
        for (id, policy) in &self.missing {
            let target = to.fields.get(id).ok_or_else(|| anyhow::anyhow!("Unknown target field {id}"))?;
            if let MissingPolicy::Constant { value } = policy {
                ensure!(target.ty.accepts(value), "Invalid default for {}", target.name);
            }
            if matches!(policy, MissingPolicy::PreserveAbsent) {
                ensure!(!target.required, "Cannot omit required field {}", target.name);
            }
        }
        for (id, source) in &from.fields {
            if let Some(target) = to.fields.get(id) {
                ensure!(source.ty == target.ty || self.transforms.contains_key(id), "Field {id} needs an explicit type transform");
            } else {
                ensure!(self.allow_drop.contains(id), "Field {id} removal requires approval");
            }
        }
        for (id, target) in &to.fields {
            if from.fields.get(id).is_none_or(|field| !field.required) {
                ensure!(self.missing.contains_key(id), "Missing semantic decision for {}", target.name);
            }
        }
        Ok(())
    }

    pub fn apply(&self, from: &Contract, to: &Contract, value: Value, mode: ValueMode) -> Result<Value> {
        from.validate_value(&value, mode)?;
        let output = self.project(from, to, value, mode)?;
        to.validate_value(&output, mode)?;
        Ok(output)
    }

    pub(crate) fn project(&self, from: &Contract, to: &Contract, value: Value, mode: ValueMode) -> Result<Value> {
        self.validate(from, to)?;
        let source = value.as_object().ok_or_else(|| anyhow::anyhow!("Expected object"))?;
        let mut output = Map::new();
        for (id, target) in &to.fields {
            if let Some(value) = from.fields.get(id).and_then(|field| source.get(&field.name)) {
                let value = match self.transforms.get(id) {
                    Some(transform) => transform.apply(&from.fields[id].ty, &target.ty, value.clone())?,
                    None => value.clone(),
                };
                output.insert(target.name.clone(), value);
            } else if mode == ValueMode::Complete {
                match self.missing.get(id) {
                    Some(MissingPolicy::Constant { value }) => { output.insert(target.name.clone(), value.clone()); }
                    Some(MissingPolicy::PreserveAbsent) => {}
                    _ => bail!("No historical value policy for {}", target.name),
                }
            }
        }
        Ok(Value::Object(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn versions() -> (Contract, Contract, Adapter) {
        let from = Contract {
            format_version: FORMAT_VERSION,
            subject: "693c4a43-d608-4a87-a79a-5629f0a1fa26".into(), revision: 0,
            fields: BTreeMap::from([(1, Field { id: 1, name: "name".into(), ty: ValueType::String, required: true })]),
            reserved_ids: BTreeSet::new(),
            field_uuids: BTreeMap::new(),
        };
        let mut to = from.clone();
        to.revision = 1;
        to.fields.get_mut(&1).unwrap().name = "display_name".into();
        to.fields.insert(2, Field { id: 2, name: "avatar".into(), ty: ValueType::Option { value: Box::new(ValueType::String) }, required: false });
        let adapter = Adapter { from_fingerprint: from.fingerprint().unwrap(), to_fingerprint: to.fingerprint().unwrap(),
            missing: BTreeMap::from([(2, MissingPolicy::Constant { value: Value::Null })]), allow_drop: BTreeSet::new(), transforms: BTreeMap::new() };
        (from, to, adapter)
    }

    #[test]
    fn schema_rename_and_complete_default_are_explicit() {
        let (from, to, adapter) = versions();
        assert_eq!(adapter.apply(&from, &to, json!({"name":"Eden"}), ValueMode::Complete).unwrap(), json!({"display_name":"Eden","avatar":null}));
        let mut unresolved = adapter;
        unresolved.missing.clear();
        assert!(unresolved.validate(&from, &to).is_err());
    }

    #[test]
    fn schema_old_patch_does_not_clear_new_field() {
        let (from, to, adapter) = versions();
        let patch = adapter.apply(&from, &to, json!({"name":"Eden"}), ValueMode::Patch).unwrap();
        assert_eq!(patch, json!({"display_name":"Eden"}));
        assert_eq!(adapter.apply(&from, &to, json!({}), ValueMode::Patch).unwrap(), json!({}));
    }

    #[test]
    fn schema_rejects_drift_unknown_fields_and_reused_ids() {
        let (from, mut to, adapter) = versions();
        assert!(adapter.apply(&from, &to, json!({"name":"Eden","admin":true}), ValueMode::Patch).is_err());
        to.revision = 2;
        assert!(adapter.validate(&from, &to).is_err());
        to.reserved_ids.insert(1);
        assert!(to.validate().is_err());
    }
}