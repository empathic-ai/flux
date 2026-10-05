use super::*;

pub struct TransformRegistration {
    pub id: &'static str,
    pub implementation: &'static [u8],
    pub apply: fn(Value) -> Result<Value>,
}

inventory::collect!(TransformRegistration);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformFixture {
    pub input: Value,
    pub output: Value,
}

fn custom_registration(id: &str, digest: &str) -> Result<&'static TransformRegistration> {
    let mut matches = inventory::iter::<TransformRegistration>.into_iter().filter(|entry| entry.id == id);
    let registration = matches.next().ok_or_else(|| anyhow::anyhow!("Unknown custom transform {id}"))?;
    ensure!(matches.next().is_none(), "Duplicate custom transform {id}");
    ensure!(format!("sha256:{:x}", Sha256::digest(registration.implementation)) == digest, "Custom transform implementation drift: {id}");
    Ok(registration)
}

fn bounded_value(value: &Value) -> Result<()> {
    let mut pending = vec![(value, 0)];
    let mut remaining = 100_000usize;
    while let Some((value, depth)) = pending.pop() {
        ensure!(depth <= 64, "Custom transform value exceeds depth limit");
        remaining = remaining.checked_sub(1).ok_or_else(|| anyhow::anyhow!("Custom transform value exceeds node limit"))?;
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::Object(values) => pending.extend(values.values().map(|value| (value, depth + 1))),
            _ => {}
        }
    }
    ensure!(serde_json::to_vec(value)?.len() <= MAX_GRAPH_BYTES, "Custom transform value exceeds byte limit");
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Transform {
    CheckedIntegerV1,
    EnumMapV1 { mapping: BTreeMap<String, String>, allow_lossy: bool },
    OptionV1 { value: Box<Transform> },
    ListV1 { item: Box<Transform> },
    MapV1 { value: Box<Transform> },
    CustomV1 { id: String, implementation_digest: String, fixtures: Vec<TransformFixture> },
}

impl Transform {
    pub fn validate(&self, from: &ValueType, to: &ValueType) -> Result<()> {
        self.validate_at(from, to, 0)
    }

    fn validate_at(&self, from: &ValueType, to: &ValueType, depth: usize) -> Result<()> {
        ensure!(depth <= 64, "Transform exceeds depth limit");
        match (self, from, to) {
            (Self::CustomV1 { id, implementation_digest, fixtures }, _, _) => {
                ensure!(!fixtures.is_empty() && fixtures.len() <= 32, "Custom transforms require one to 32 fixtures");
                let registration = custom_registration(id, implementation_digest)?;
                let key = Sha256::digest(serde_json::to_vec(&(self, from, to))?).to_vec();
                static VERIFIED: std::sync::OnceLock<std::sync::Mutex<BTreeSet<Vec<u8>>>> = std::sync::OnceLock::new();
                let verified = VERIFIED.get_or_init(Default::default);
                if verified.lock().map_err(|_| anyhow::anyhow!("Transform verification cache poisoned"))?.contains(&key) {
                    return Ok(());
                }
                for fixture in fixtures {
                    bounded_value(&fixture.input)?;
                    bounded_value(&fixture.output)?;
                    ensure!(from.accepts(&fixture.input) && to.accepts(&fixture.output), "Custom transform fixture does not match its field contracts");
                    let output = (registration.apply)(fixture.input.clone())?;
                    bounded_value(&output)?;
                    ensure!(output == fixture.output, "Custom transform fixture failed: {id}");
                }
                let mut verified = verified.lock().map_err(|_| anyhow::anyhow!("Transform verification cache poisoned"))?;
                if verified.len() < 1024 { verified.insert(key); }
                Ok(())
            }
            (Self::CheckedIntegerV1, ValueType::I64 | ValueType::U64 | ValueType::Integer { .. }, ValueType::I64 | ValueType::U64 | ValueType::Integer { .. }) => Ok(()),
            (Self::EnumMapV1 { mapping, allow_lossy }, ValueType::StringEnum { variants: source }, ValueType::StringEnum { variants: target }) => {
                ensure!(mapping.keys().collect::<BTreeSet<_>>() == source.iter().collect(), "Enum mapping must cover exactly the source variants");
                ensure!(mapping.values().all(|value| target.contains(value)), "Enum mapping has an unknown target variant");
                ensure!(*allow_lossy || mapping.values().collect::<BTreeSet<_>>().len() == mapping.len(), "Many-to-one enum mapping requires explicit loss approval");
                Ok(())
            }
            (Self::OptionV1 { value }, ValueType::Option { value: source }, ValueType::Option { value: target })
            | (Self::MapV1 { value }, ValueType::Map { value: source }, ValueType::Map { value: target })
            | (Self::MapV1 { value }, ValueType::UuidMap { value: source }, ValueType::UuidMap { value: target }) => value.validate_at(source, target, depth + 1),
            (Self::ListV1 { item }, ValueType::List { item: source }, ValueType::List { item: target }) => item.validate_at(source, target, depth + 1),
            _ => bail!("Transform does not match source and target value types"),
        }
    }

    pub fn apply(&self, from: &ValueType, to: &ValueType, value: Value) -> Result<Value> {
        self.validate(from, to)?;
        let mut remaining = 100_000;
        self.apply_at(from, to, value, &mut remaining)
    }

    fn apply_at(&self, from: &ValueType, to: &ValueType, value: Value, remaining: &mut usize) -> Result<Value> {
        *remaining = remaining.checked_sub(1).ok_or_else(|| anyhow::anyhow!("Transform exceeds node limit"))?;
        match (self, from, to, value) {
            (Self::CustomV1 { id, implementation_digest, .. }, _, _, value) => {
                bounded_value(&value)?;
                ensure!(from.accepts(&value), "Custom transform source mismatch");
                let output = (custom_registration(id, implementation_digest)?.apply)(value)?;
                bounded_value(&output)?;
                ensure!(to.accepts(&output), "Custom transform target mismatch");
                Ok(output)
            }
            (Self::CheckedIntegerV1, _, _, value) => {
                ensure!(from.accepts(&value) && to.accepts(&value), "Integer conversion is out of range");
                Ok(value)
            }
            (Self::EnumMapV1 { mapping, .. }, _, _, Value::String(value)) => {
                Ok(Value::String(mapping.get(&value).ok_or_else(|| anyhow::anyhow!("Unmapped enum value"))?.clone()))
            }
            (Self::OptionV1 { .. }, _, _, Value::Null) => Ok(Value::Null),
            (Self::OptionV1 { value: transform }, ValueType::Option { value: source }, ValueType::Option { value: target }, value) => transform.apply_at(source, target, value, remaining),
            (Self::ListV1 { item }, ValueType::List { item: source }, ValueType::List { item: target }, Value::Array(values)) => {
                Ok(Value::Array(values.into_iter().map(|value| item.apply_at(source, target, value, remaining)).collect::<Result<_>>()?))
            }
            (Self::MapV1 { value: transform }, ValueType::Map { value: source } | ValueType::UuidMap { value: source }, ValueType::Map { value: target } | ValueType::UuidMap { value: target }, Value::Object(values)) => {
                ensure!(!matches!(from, ValueType::UuidMap { .. }) || uuid_keys_valid(&values), "Invalid UUID map keys");
                Ok(Value::Object(values.into_iter().map(|(key, value)| Ok((key, transform.apply_at(source, target, value, remaining)?))).collect::<Result<_>>()?))
            }
            _ => bail!("Invalid transform input"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    inventory::submit! {
        TransformRegistration {
            id: "test.string_length.v1",
            implementation: include_bytes!("transform.rs"),
            apply: |value| Ok(json!(value.as_str().ok_or_else(|| anyhow::anyhow!("Expected string"))?.len())),
        }
    }

    #[test]
    fn custom_transforms_pin_implementation_and_require_valid_fixtures() {
        let digest = format!("sha256:{:x}", Sha256::digest(include_bytes!("transform.rs")));
        let fixture = TransformFixture { input: json!("hello"), output: json!(5) };
        let transform = Transform::CustomV1 { id: "test.string_length.v1".into(), implementation_digest: digest.clone(), fixtures: vec![fixture.clone()] };
        assert_eq!(transform.apply(&ValueType::String, &ValueType::U64, json!("abc")).unwrap(), json!(3));
        for invalid in [
            Transform::CustomV1 { id: "unknown".into(), implementation_digest: digest.clone(), fixtures: vec![fixture.clone()] },
            Transform::CustomV1 { id: "test.string_length.v1".into(), implementation_digest: "changed".into(), fixtures: vec![fixture.clone()] },
            Transform::CustomV1 { id: "test.string_length.v1".into(), implementation_digest: digest.clone(), fixtures: vec![] },
            Transform::CustomV1 { id: "test.string_length.v1".into(), implementation_digest: digest, fixtures: vec![TransformFixture { input: fixture.input, output: json!(6) }] },
        ] {
            assert!(invalid.validate(&ValueType::String, &ValueType::U64).is_err());
        }
        assert!(transform.apply(&ValueType::String, &ValueType::U64, json!("x".repeat(MAX_GRAPH_BYTES))).is_err());
        assert!(transform.apply(&ValueType::String, &ValueType::Integer { min: 0, max: 5 }, json!("too long")).is_err());
    }

    #[test]
    fn transforms_check_ranges_enum_coverage_and_loss() {
        let narrow = ValueType::Integer { min: 0, max: 255 };
        assert_eq!(Transform::CheckedIntegerV1.apply(&ValueType::I64, &narrow, json!(255)).unwrap(), json!(255));
        for value in [json!(-1), json!(256), json!(1.5), json!("1")] {
            assert!(Transform::CheckedIntegerV1.apply(&ValueType::I64, &narrow, value).is_err());
        }
        let source = ValueType::StringEnum { variants: ["first".into(), "second".into()].into() };
        let target = ValueType::StringEnum { variants: ["combined".into()].into() };
        let mapping: BTreeMap<String, String> = [("first".into(), "combined".into()), ("second".into(), "combined".into())].into();
        assert!(Transform::EnumMapV1 { mapping: mapping.clone(), allow_lossy: false }.validate(&source, &target).is_err());
        let approved = Transform::EnumMapV1 { mapping, allow_lossy: true };
        assert_eq!(approved.apply(&source, &target, json!("first")).unwrap(), json!("combined"));
        assert!(approved.apply(&source, &target, json!("unknown")).is_err());
        assert!(Transform::EnumMapV1 { mapping: BTreeMap::new(), allow_lossy: true }.validate(&source, &target).is_err());
    }

    #[test]
    fn transforms_preserve_absent_patch_fields_and_old_artifact_bytes() {
        let from = Contract {
            format_version: FORMAT_VERSION, subject: uuid::Uuid::new_v4().to_string(), revision: 0,
            fields: [(1, Field { id: 1, name: "count".into(), ty: ValueType::I64, required: true })].into(),
            reserved_ids: BTreeSet::new(), field_uuids: BTreeMap::new(),
        };
        let mut to = from.clone();
        to.revision = 1;
        to.fields.get_mut(&1).unwrap().ty = ValueType::Integer { min: 0, max: 255 };
        let mut adapter = Adapter {
            from_fingerprint: from.fingerprint().unwrap(), to_fingerprint: to.fingerprint().unwrap(),
            missing: BTreeMap::new(), allow_drop: BTreeSet::new(), transforms: BTreeMap::new(),
        };
        assert!(!serde_json::to_value(&adapter).unwrap().as_object().unwrap().contains_key("transforms"));
        assert!(adapter.validate(&from, &to).is_err());
        adapter.transforms.insert(1, Transform::CheckedIntegerV1);
        assert_eq!(adapter.apply(&from, &to, json!({}), ValueMode::Patch).unwrap(), json!({}));
        assert_eq!(adapter.apply(&from, &to, json!({"count": 7}), ValueMode::Complete).unwrap(), json!({"count": 7}));
        assert!(adapter.apply(&from, &to, json!({"count": -1}), ValueMode::Complete).is_err());
    }

    #[test]
    fn authored_transforms_are_required_and_immutable_after_publication() {
        let mut descriptor = DiscoveredContract {
            name: "Counter".into(), rename_from: None, subject: None, database: false, wire: true,
            fields: vec![DiscoveredField { name: "count".into(), uuid: None, rename_from: None, ty: ValueType::I64, required: true }],
        };
        let catalog = Catalog::init(&[(descriptor.name.clone(), descriptor.clone())].into()).unwrap();
        descriptor.fields[0].ty = ValueType::Integer { min: 0, max: 255 };
        let mut draft = catalog.draft("checked-counter".into(), &[(descriptor.name.clone(), descriptor)].into()).unwrap();
        assert!(catalog.finalize(&draft).is_err());
        let update = draft.updates.get_mut("Counter").unwrap();
        update.migration.upgrade.transforms.insert(1, Transform::CheckedIntegerV1);
        let published = catalog.finalize(&draft).unwrap();
        published.verify_prefix(&catalog).unwrap();
        let history = &published.roots["Counter"].history;
        let adapter = &history.migrations[0].upgrade;
        assert_eq!(adapter.apply(&history.contracts[0], &history.contracts[1], json!({"count": 9}), ValueMode::Complete).unwrap(), json!({"count": 9}));
        let mut changed = published.clone();
        changed.roots.get_mut("Counter").unwrap().history.migrations[0].upgrade.transforms.clear();
        assert!(changed.verify_prefix(&published).is_err());
        let legacy = json!({"from_fingerprint":"source", "to_fingerprint":"target", "missing":{}, "allow_drop":[]});
        let restored: Adapter = serde_json::from_value(legacy.clone()).unwrap();
        assert!(restored.transforms.is_empty());
        assert_eq!(serde_json::to_value(restored).unwrap(), legacy);
    }

    #[test]
    fn transforms_map_containers_and_preserve_null() {
        let source = ValueType::Option { value: Box::new(ValueType::List { item: Box::new(ValueType::I64) }) };
        let target = ValueType::Option { value: Box::new(ValueType::List { item: Box::new(ValueType::U64) }) };
        let transform = Transform::OptionV1 { value: Box::new(Transform::ListV1 { item: Box::new(Transform::CheckedIntegerV1) }) };
        assert_eq!(transform.apply(&source, &target, json!(null)).unwrap(), json!(null));
        assert_eq!(transform.apply(&source, &target, json!([0, 5])).unwrap(), json!([0, 5]));
        assert!(transform.apply(&source, &target, json!([0, -5])).is_err());
    }
}