use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Change {
    Added { field: Field },
    Removed { field: Field },
    Changed { before: Field, after: Field },
}

pub fn diff(from: &Contract, to: &Contract) -> Result<Vec<Change>> {
    from.validate()?;
    to.validate()?;
    ensure!(from.subject == to.subject, "Subject identity changed");
    let mut changes = Vec::new();
    for (id, before) in &from.fields {
        match to.fields.get(id) {
            None => changes.push(Change::Removed { field: before.clone() }),
            Some(after) if before != after => changes.push(Change::Changed { before: before.clone(), after: after.clone() }),
            _ => {}
        }
    }
    for (id, field) in &to.fields {
        if !from.fields.contains_key(id) {
            changes.push(Change::Added { field: field.clone() });
        }
    }
    Ok(changes)
}

pub fn validate_successor(from: &Contract, to: &Contract) -> Result<()> {
    from.validate()?;
    to.validate()?;
    ensure!(from.subject == to.subject, "Subject identity changed");
    ensure!(from.revision.checked_add(1) == Some(to.revision), "Non-adjacent contract revisions");
    ensure!(from.reserved_ids.is_subset(&to.reserved_ids), "Field tombstones were removed");
    for (uuid, id) in &from.field_uuids {
        ensure!(to.field_uuids.get(uuid) == Some(id), "Published field UUID mapping changed");
    }
    for id in from.fields.keys() {
        ensure!(to.fields.contains_key(id) || to.reserved_ids.contains(id), "Removed field {id} must be reserved");
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub id: String,
    pub from_revision: u32,
    pub upgrade: Adapter,
    pub downgrade: Option<Adapter>,
}

impl Migration {
    pub fn fingerprint(&self) -> Result<String> {
        Ok(format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub format_version: u16,
    pub contracts: Vec<Contract>,
    pub migrations: Vec<Migration>,
}

impl History {
    pub fn new(baseline: Contract) -> Result<Self> {
        let history = Self { format_version: FORMAT_VERSION, contracts: vec![baseline], migrations: Vec::new() };
        history.validate()?;
        Ok(history)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.format_version == FORMAT_VERSION, "Unsupported history format");
        ensure!(!self.contracts.is_empty(), "Missing baseline contract");
        ensure!(self.contracts[0].revision == 0, "Baseline must be revision zero");
        ensure!(self.contracts.len() == self.migrations.len() + 1, "Incomplete contract history");
        self.contracts[0].validate()?;
        let mut ids = BTreeSet::new();
        for (index, migration) in self.migrations.iter().enumerate() {
            ensure!(!migration.id.is_empty() && ids.insert(&migration.id), "Duplicate or empty migration ID");
            let from = &self.contracts[index];
            let to = &self.contracts[index + 1];
            ensure!(migration.from_revision == from.revision, "Migration ordering mismatch");
            validate_successor(from, to)?;
            migration.upgrade.validate(from, to)?;
            if let Some(downgrade) = &migration.downgrade {
                downgrade.validate(to, from)?;
            }
        }
        Ok(())
    }

    pub fn append(&mut self, contract: Contract, migration: Migration) -> Result<()> {
        let mut candidate = self.clone();
        candidate.contracts.push(contract);
        candidate.migrations.push(migration);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn verify_prefix(&self, published: &Self) -> Result<()> {
        self.validate()?;
        published.validate()?;
        ensure!(self.contracts.starts_with(&published.contracts), "Published contract history changed");
        ensure!(self.migrations.starts_with(&published.migrations), "Published migration history changed");
        Ok(())
    }

    pub fn adapt(&self, from_revision: u32, to_revision: u32, mut value: Value, mode: ValueMode) -> Result<Value> {
        self.validate()?;
        let from_index = usize::try_from(from_revision)?;
        let to_index = usize::try_from(to_revision)?;
        let from = self.contracts.get(from_index).ok_or_else(|| anyhow::anyhow!("Unknown writer revision"))?;
        ensure!(self.contracts.get(to_index).is_some(), "Unknown reader revision");
        from.validate_value(&value, mode)?;
        if from_index < to_index {
            for index in from_index..to_index {
                value = self.migrations[index].upgrade.apply(&self.contracts[index], &self.contracts[index + 1], value, mode)?;
            }
        } else {
            for index in (to_index..from_index).rev() {
                let adapter = self.migrations[index].downgrade.as_ref().ok_or_else(|| anyhow::anyhow!("Downgrade unsupported at revision {}", index + 1))?;
                value = adapter.apply(&self.contracts[index + 1], &self.contracts[index], value, mode)?;
            }
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn baseline() -> Contract {
        Contract { format_version: FORMAT_VERSION, subject: "693c4a43-d608-4a87-a79a-5629f0a1fa26".into(), revision: 0,
            fields: BTreeMap::from([(1, Field { id: 1, name: "name".into(), ty: ValueType::String, required: true })]), reserved_ids: BTreeSet::new(), field_uuids: BTreeMap::new() }
    }

    #[test]
    fn history_requires_tombstones_and_retains_them() {
        let first = baseline();
        let mut second = first.clone();
        second.revision = 1;
        second.fields.clear();
        assert!(validate_successor(&first, &second).is_err());
        second.reserved_ids.insert(1);
        validate_successor(&first, &second).unwrap();
        let mut third = first.clone();
        third.revision = 2;
        assert!(validate_successor(&second, &third).is_err());
    }

    #[test]
    fn history_validates_composition_and_published_prefix() {
        let first = baseline();
        let mut second = first.clone();
        second.revision = 1;
        second.fields.get_mut(&1).unwrap().name = "display_name".into();
        let upgrade = Adapter { from_fingerprint: first.fingerprint().unwrap(), to_fingerprint: second.fingerprint().unwrap(), missing: BTreeMap::new(), allow_drop: BTreeSet::new(), transforms: BTreeMap::new() };
        let mut history = History::new(first).unwrap();
        let published = history.clone();
        history.append(second, Migration { id: "rename_name".into(), from_revision: 0, upgrade, downgrade: None }).unwrap();
        history.verify_prefix(&published).unwrap();
        assert_eq!(history.adapt(0, 1, json!({"name":"Eden"}), ValueMode::Complete).unwrap(), json!({"display_name":"Eden"}));
        assert!(history.adapt(1, 0, json!({"display_name":"Eden"}), ValueMode::Complete).is_err());
        assert!(history.adapt(9, 1, json!({}), ValueMode::Patch).is_err());
        history.migrations[0].upgrade.to_fingerprint = "changed".into();
        assert!(history.validate().is_err());
    }
}