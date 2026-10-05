use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackedContract {
    pub database: bool,
    pub wire: bool,
    pub history: History,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub database_graphs: Vec<GraphRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub format_version: u16,
    pub roots: BTreeMap<String, TrackedContract>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractUpdate {
    pub target: Contract,
    pub migration: Migration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub format_version: u16,
    pub id: String,
    pub base_fingerprint: String,
    pub additions: BTreeMap<String, TrackedContract>,
    pub updates: BTreeMap<String, ContractUpdate>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
}

impl Catalog {
    pub fn named_root(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(String::as_str)
            .or_else(|| self.roots.get_key_value(name).map(|(key, _)| key.as_str()))
    }

    fn resolve_roots<'root>(&self, discovered: &'root BTreeMap<String, DiscoveredContract>) -> Result<BTreeMap<String, &'root DiscoveredContract>> {
        let mut resolved = BTreeMap::new();
        for descriptor in discovered.values() {
            let named = self.named_root(&descriptor.name);
            let renamed = descriptor.rename_from.as_deref().and_then(|name| self.named_root(name));
            ensure!(descriptor.rename_from.is_none() || renamed.is_some(), "Unknown type rename source for {}", descriptor.name);
            if let (Some(named), Some(renamed)) = (named, renamed) {
                ensure!(named == renamed, "Type {} and rename_from identify different published roots", descriptor.name);
            }
            let previous_key = renamed.or(named);
            let key = if let Some(subject) = &descriptor.subject {
                let subject = uuid::Uuid::parse_str(subject)?.to_string();
                if let Some(previous) = previous_key.and_then(|key| self.roots.get(key)) {
                    let expected = &previous.history.contracts[0].subject;
                    ensure!(&subject == expected, "Schema type {} identity changed: expected UUID {}, supplied UUID {}", descriptor.name, expected, subject);
                }
                self.roots.iter().find(|(_, root)| root.history.contracts[0].subject == subject)
                    .map(|(key, _)| key.clone()).unwrap_or_else(|| descriptor.name.clone())
            } else { previous_key.map(str::to_owned).unwrap_or_else(|| descriptor.name.clone()) };
            ensure!(resolved.insert(key, descriptor).is_none(), "Duplicate schema root identity or storage key");
        }
        Ok(resolved)
    }

    pub(crate) fn resolve_descriptors(&self, discovered: &BTreeMap<String, DiscoveredContract>) -> Result<BTreeMap<String, DiscoveredContract>> {
        let mut resolved: BTreeMap<_, _> = self.resolve_roots(discovered)?.into_iter().map(|(key, descriptor)| (key, descriptor.clone())).collect();
        let mut subjects = BTreeMap::new();
        for (key, descriptor) in &mut resolved {
            let subject = descriptor.subject.as_deref().map(uuid::Uuid::parse_str).transpose()?.map(|id| id.to_string())
                .or_else(|| self.roots.get(key).map(|root| root.history.contracts[0].subject.clone()))
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            subjects.insert(descriptor.name.clone(), subject.clone());
            subjects.insert(subject.clone(), subject.clone());
            descriptor.subject = Some(subject);
        }
        for descriptor in resolved.values_mut() {
            for field in &mut descriptor.fields { field.ty.resolve(&subjects)?; }
        }
        Ok(resolved)
    }

    pub fn init(discovered: &BTreeMap<String, DiscoveredContract>) -> Result<Self> {
        ensure!(!discovered.is_empty(), "No schema roots discovered; derive Schema and mark database or wire roots");
        let empty = Self { format_version: FORMAT_VERSION, roots: BTreeMap::new(), aliases: BTreeMap::new() };
        let resolved = empty.resolve_descriptors(discovered)?;
        let roots = resolved.iter().map(|(name, descriptor)| {
            Ok((name.clone(), TrackedContract { database: descriptor.database, wire: descriptor.wire,
                history: History::new(descriptor.reconcile(None)?)?, database_graphs: Vec::new() }))
        }).collect::<Result<_>>()?;
        let aliases = resolved.iter().filter(|(key, descriptor)| **key != descriptor.name)
            .map(|(key, descriptor)| (descriptor.name.clone(), key.clone())).collect();
        let mut catalog = Self { format_version: FORMAT_VERSION, roots, aliases };
        catalog.pin_database_graphs()?;
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(self.format_version == FORMAT_VERSION, "Unsupported catalog format");
        let mut subjects = BTreeSet::new();
        for (name, tracked) in &self.roots {
            ensure!(!name.is_empty(), "Invalid root {name}");
            tracked.history.validate()?;
            ensure!(subjects.insert(&tracked.history.contracts[0].subject), "Duplicate subject identity");
        }
        for tracked in self.roots.values() {
            for contract in &tracked.history.contracts {
                let mut references = BTreeSet::new();
                for field in contract.fields.values() { field.ty.references(&mut references); }
                for subject in references { ensure!(subjects.contains(&subject), "Missing nested schema subject {subject}"); }
            }
        }
        for (alias, key) in &self.aliases {
            ensure!(!alias.is_empty() && self.roots.contains_key(key), "Invalid schema alias {alias}");
            ensure!(!self.roots.contains_key(alias) || alias == key, "Schema alias {alias} shadows another root");
        }
        for tracked in self.roots.values() {
            ensure!(tracked.database || tracked.database_graphs.is_empty(), "Only database roots have database graph history");
            let mut previous: Option<&GraphRef> = None;
            for graph in &tracked.database_graphs {
                ensure!(graph.root == tracked.history.contracts[0].subject, "Database graph root mismatch");
                graph.validate_contracts(self)?;
                if let Some(previous) = previous {
                    ensure!(previous != graph, "Duplicate database graph revision");
                    for (subject, before) in &previous.contracts {
                        if let Some(after) = graph.contracts.get(subject) {
                            ensure!(after.revision >= before.revision, "Database graph history goes backwards");
                        }
                    }
                }
                previous = Some(graph);
            }
        }
        Ok(())
    }

    fn pin_database_graphs(&mut self) -> Result<()> {
        let names: Vec<_> = self.roots.iter().filter(|(_, root)| root.database).map(|(name, _)| name.clone()).collect();
        for name in names {
            let graph = self.graph_ref(&name)?;
            let root = &mut self.roots.get_mut(&name).unwrap();
            let mut references = BTreeSet::new();
            for field in root.history.contracts.last().unwrap().fields.values() { field.ty.references(&mut references); }
            if (!references.is_empty() || !root.database_graphs.is_empty()) && root.database_graphs.last() != Some(&graph) {
                root.database_graphs.push(graph);
            }
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("sha256:{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn verify_prefix(&self, published: &Self) -> Result<()> {
        self.validate()?;
        published.validate()?;
        for (name, root) in &published.roots {
            let current = self.roots.get(name).ok_or_else(|| anyhow::anyhow!("Published root {name} removed"))?;
            ensure!(current.database == root.database && current.wire == root.wire, "Published capabilities changed for {name}");
            current.history.verify_prefix(&root.history)?;
            ensure!(current.database_graphs.starts_with(&root.database_graphs), "Published database graph history changed for {name}");
        }
        for (alias, key) in &published.aliases {
            ensure!(self.named_root(alias) == Some(key.as_str()), "Published schema alias {alias} was removed or reassigned");
        }
        Ok(())
    }

    pub fn draft(&self, id: String, discovered: &BTreeMap<String, DiscoveredContract>) -> Result<Draft> {
        ensure!(!id.is_empty(), "Empty change ID");
        let mut draft = Draft { format_version: FORMAT_VERSION, id: id.clone(), base_fingerprint: self.fingerprint()?, additions: BTreeMap::new(), updates: BTreeMap::new(), aliases: BTreeMap::new() };
        let discovered = self.resolve_descriptors(discovered)?;
        for (name, tracked) in &self.roots {
            ensure!(discovered.contains_key(name) || !(tracked.database || tracked.wire), "Root {name} disappeared; explicit retirement is required");
        }
        for (name, descriptor) in discovered {
            if self.named_root(&descriptor.name) != Some(name.as_str()) && descriptor.name != name {
                ensure!(draft.aliases.insert(descriptor.name.clone(), name.clone()).is_none(), "Duplicate schema declaration name {}", descriptor.name);
            }
            if let Some(previous) = self.roots.get(&name) {
                ensure!(previous.database == descriptor.database && previous.wire == descriptor.wire, "Root {name} capability changed; explicit deployment plan required");
                let source = previous.history.contracts.last().unwrap();
                let target = descriptor.reconcile(Some(source))?;
                if &target != source {
                    let mut missing = BTreeMap::new();
                    for (field_id, field) in &target.fields {
                        if source.fields.get(field_id).is_some_and(|old| old == field && !old.required) {
                            missing.insert(*field_id, MissingPolicy::PreserveAbsent);
                        }
                    }
                    let upgrade = Adapter { from_fingerprint: source.fingerprint()?, to_fingerprint: target.fingerprint()?, missing, allow_drop: BTreeSet::new(), transforms: BTreeMap::new() };
                    draft.updates.insert(name.clone(), ContractUpdate { target, migration: Migration { id: id.clone(), from_revision: source.revision, upgrade, downgrade: None } });
                }
            } else {
                draft.additions.insert(name.clone(), TrackedContract { database: descriptor.database, wire: descriptor.wire, history: History::new(descriptor.reconcile(None)?)?, database_graphs: Vec::new() });
            }
        }
        Ok(draft)
    }

    pub fn finalize(&self, draft: &Draft) -> Result<Self> {
        ensure!(draft.format_version == FORMAT_VERSION, "Unsupported draft format");
        ensure!(draft.base_fingerprint == self.fingerprint()?, "Draft is stale; published history changed");
        ensure!(!draft.id.is_empty(), "Empty change ID");
        ensure!(!draft.is_empty(), "Empty migration draft");
        let mut result = self.clone();
        for (name, added) in &draft.additions {
            ensure!(!result.roots.contains_key(name), "Root {name} already exists");
            ensure!(added.history.migrations.is_empty(), "A new root must start at baseline");
            ensure!(added.database_graphs.is_empty(), "Database graph snapshots are generated during finalization");
            result.roots.insert(name.clone(), added.clone());
        }
        for (name, update) in &draft.updates {
            ensure!(update.migration.id == draft.id, "Migration ID differs from draft");
            ensure!(!draft.additions.contains_key(name), "Root both added and updated");
            let tracked = result.roots.get_mut(name).ok_or_else(|| anyhow::anyhow!("Unknown root {name}"))?;
            tracked.history.append(update.target.clone(), update.migration.clone())?;
        }
        for (alias, key) in &draft.aliases {
            if let Some(previous) = result.named_root(alias) {
                ensure!(previous == key, "Published schema alias {alias} cannot be reassigned");
            }
            result.aliases.insert(alias.clone(), key.clone());
        }
        result.pin_database_graphs()?;
        result.validate()?;
        Ok(result)
    }

    pub fn check(&self, discovered: &BTreeMap<String, DiscoveredContract>) -> Result<()> {
        let draft = self.draft("check".into(), discovered)?;
        ensure!(draft.is_empty(), "Rust schema differs from published catalog; create and review a migration draft");
        Ok(())
    }
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty() && self.additions.is_empty() && self.aliases.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_catalog_aliases_cannot_disappear_or_move() {
        let descriptor = |name: &str| DiscoveredContract { name: name.into(), rename_from: None, subject: None, database: true, wire: false, fields: Vec::new() };
        let mut published = Catalog::init(&BTreeMap::from([("First".into(), descriptor("First")), ("Second".into(), descriptor("Second"))])).unwrap();
        published.aliases.insert("Renamed".into(), "First".into());
        let mut next = published.clone();
        next.aliases.insert("Another".into(), "First".into());
        next.verify_prefix(&published).unwrap();
        next.aliases.remove("Renamed");
        assert!(next.verify_prefix(&published).unwrap_err().to_string().contains("Renamed"));
        next.aliases.insert("Renamed".into(), "Second".into());
        assert!(next.verify_prefix(&published).is_err());
        next = published.clone();
        next.roots.get_mut("First").unwrap().wire = true;
        assert!(next.verify_prefix(&published).is_err());
    }

    #[test]
    fn type_rename_survives_hint_removal_and_uuid_toggles() {
        let mut descriptor = DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: None, database: true, wire: false, fields: Vec::new(),
        };
        let discovered = |descriptor: &DiscoveredContract| BTreeMap::from([(descriptor.name.clone(), descriptor.clone())]);
        let original = Catalog::init(&discovered(&descriptor)).unwrap();
        descriptor.name = "Account".into();
        descriptor.rename_from = Some("Profile".into());
        let draft = original.draft("rename".into(), &discovered(&descriptor)).unwrap();
        assert!(draft.updates.is_empty());
        assert!(draft.additions.is_empty());
        let renamed = original.finalize(&draft).unwrap();
        assert_eq!(original.roots, renamed.roots);
        assert_eq!(renamed.named_root("Account"), Some("Profile"));
        descriptor.rename_from = None;
        renamed.check(&discovered(&descriptor)).unwrap();
        descriptor.subject = Some(original.roots["Profile"].history.contracts[0].subject.clone());
        renamed.check(&discovered(&descriptor)).unwrap();
        descriptor.subject = None;
        renamed.check(&discovered(&descriptor)).unwrap();
        descriptor.subject = Some(uuid::Uuid::new_v4().to_string());
        assert!(renamed.check(&discovered(&descriptor)).unwrap_err().to_string().contains("expected UUID"));
    }

    #[test]
    fn uuid_root_keeps_table_when_attribute_is_removed() {
        let mut descriptor = DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: Some(uuid::Uuid::new_v4().to_string()), database: true, wire: false, fields: Vec::new(),
        };
        let original = Catalog::init(&BTreeMap::from([("Profile".into(), descriptor.clone())])).unwrap();
        let table = original.named_root("Profile").unwrap();
        assert_eq!(table, "Profile");
        descriptor.subject = None;
        original.check(&BTreeMap::from([("Profile".into(), descriptor)])).unwrap();
    }

    #[test]
    fn explicit_uuid_preserves_published_keys_across_renames() {
        let mut descriptor = DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: Some(uuid::Uuid::new_v4().to_string()), database: true, wire: false, fields: Vec::new(),
        };
        let original = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        assert_eq!(original.roots["Profile"].history.contracts[0].subject, *descriptor.subject.as_ref().unwrap());
        for key in ["Profile".to_owned(), format!("schema_{}", descriptor.subject.as_ref().unwrap().replace('-', ""))] {
            let mut published = original.clone();
            let root = published.roots.remove("Profile").unwrap();
            published.roots.insert(key.clone(), root);
            published.aliases.insert("Profile".into(), key.clone());
            descriptor.name = "Account".into();
            let draft = published.draft("rename".into(), &[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
            assert!(draft.additions.is_empty() && draft.updates.is_empty());
            let current = published.finalize(&draft).unwrap();
            assert_eq!(current.named_root("Account"), Some(key.as_str()));
            current.verify_prefix(&published).unwrap();
        }
    }

    #[test]
    fn draft_identity_error_reports_catalog_uuid() {
        let mut discovered = BTreeMap::from([("Profile".into(), DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: None, database: true, wire: false, fields: Vec::new(),
        })]);
        let catalog = Catalog::init(&discovered).unwrap();
        let expected = &catalog.roots["Profile"].history.contracts[0].subject;
        let supplied = uuid::Uuid::new_v4().to_string();
        discovered.get_mut("Profile").unwrap().subject = Some(supplied.clone());
        let error = catalog.draft("change".into(), &discovered).unwrap_err().to_string();
        assert!(error.contains("Schema type Profile"));
        assert!(error.contains(&format!("expected UUID {expected}, supplied UUID {supplied}")));
        discovered.get_mut("Profile").unwrap().subject = Some(expected.clone());
        catalog.check(&discovered).unwrap();
    }

    #[test]
    fn drafts_require_semantics_and_refuse_stale_history() {
        let mut discovered = BTreeMap::from([("Profile".into(), DiscoveredContract { name: "Profile".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![DiscoveredField { name: "name".into(), uuid: None, rename_from: None, ty: ValueType::String, required: true }] })]);
        let initial = Catalog::init(&discovered).unwrap();
        initial.check(&discovered).unwrap();
        discovered.get_mut("Profile").unwrap().fields.push(DiscoveredField { name: "avatar".into(), uuid: None, rename_from: None,
            ty: ValueType::Option { value: Box::new(ValueType::String) }, required: false });
        assert!(initial.check(&discovered).is_err());
        let mut draft = initial.draft("add_avatar".into(), &discovered).unwrap();
        assert!(initial.finalize(&draft).is_err());
        draft.updates.get_mut("Profile").unwrap().migration.upgrade.missing.insert(2, MissingPolicy::PreserveAbsent);
        let published = initial.finalize(&draft).unwrap();
        published.check(&discovered).unwrap();
        assert!(published.finalize(&draft).is_err());
        assert_eq!(initial.roots["Profile"].history.contracts.len(), 1);
    }
}