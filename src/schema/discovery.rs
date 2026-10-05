use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredField {
    pub name: String,
    #[serde(default)]
    pub uuid: Option<String>,
    pub rename_from: Option<String>,
    pub ty: ValueType,
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveredContract {
    pub name: String,
    #[serde(default)]
    pub rename_from: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    pub database: bool,
    pub wire: bool,
    pub fields: Vec<DiscoveredField>,
}

pub trait Schema {
    fn describe() -> DiscoveredContract;
}

pub trait SchemaValueType {
    fn value_type() -> ValueType;
    fn accepts_missing() -> bool { false }
}

impl SchemaValueType for String { fn value_type() -> ValueType { ValueType::String } }
impl SchemaValueType for uuid::Uuid { fn value_type() -> ValueType { ValueType::Uuid } }
impl<T: SchemaValueType> SchemaValueType for Box<T> {
    fn value_type() -> ValueType { T::value_type() }
    fn accepts_missing() -> bool { T::accepts_missing() }
}
#[cfg(feature = "bevy_reflect")]
impl SchemaValueType for crate::prelude::Id { fn value_type() -> ValueType { ValueType::Uuid } }
impl SchemaValueType for bool { fn value_type() -> ValueType { ValueType::Bool } }
impl SchemaValueType for i64 { fn value_type() -> ValueType { ValueType::I64 } }
impl SchemaValueType for u64 { fn value_type() -> ValueType { ValueType::U64 } }
impl SchemaValueType for f32 { fn value_type() -> ValueType { ValueType::F32 } }
impl SchemaValueType for f64 { fn value_type() -> ValueType { ValueType::F64 } }
macro_rules! bounded_integer {
    ($($ty:ty),*) => { $(impl SchemaValueType for $ty {
        fn value_type() -> ValueType { ValueType::Integer { min: <$ty>::MIN as i64, max: <$ty>::MAX as u64 } }
    })* };
}
bounded_integer!(i8, i16, i32, u8, u16, u32);

impl<T: SchemaValueType> SchemaValueType for BTreeMap<String, T> {
    fn value_type() -> ValueType { ValueType::Map { value: Box::new(T::value_type()) } }
}
impl<T: SchemaValueType, Hasher> SchemaValueType for std::collections::HashMap<String, T, Hasher> {
    fn value_type() -> ValueType { ValueType::Map { value: Box::new(T::value_type()) } }
}
impl<T: SchemaValueType> SchemaValueType for BTreeMap<uuid::Uuid, T> {
    fn value_type() -> ValueType { ValueType::UuidMap { value: Box::new(T::value_type()) } }
}
impl<T: SchemaValueType, Hasher> SchemaValueType for std::collections::HashMap<uuid::Uuid, T, Hasher> {
    fn value_type() -> ValueType { ValueType::UuidMap { value: Box::new(T::value_type()) } }
}
impl<T: SchemaValueType, Hasher> SchemaValueType for bevy_platform::collections::HashMap<uuid::Uuid, T, Hasher> {
    fn value_type() -> ValueType { ValueType::UuidMap { value: Box::new(T::value_type()) } }
}
impl<T: SchemaValueType> SchemaValueType for Option<T> {
    fn value_type() -> ValueType { ValueType::Option { value: Box::new(T::value_type()) } }
    fn accepts_missing() -> bool { true }
}
impl<T: SchemaValueType> SchemaValueType for Vec<T> {
    fn value_type() -> ValueType { ValueType::List { item: Box::new(T::value_type()) } }
}

pub struct Registration {
    pub describe: fn() -> DiscoveredContract,
    pub rust_type: fn() -> &'static str,
}
inventory::collect!(Registration);

pub fn discovered_roots() -> Result<BTreeMap<String, DiscoveredContract>> {
    let mut roots = BTreeMap::new();
    let mut registered = BTreeMap::new();
    for registration in inventory::iter::<Registration> {
        let contract = (registration.describe)();
        ensure!(registered.insert(contract.name.clone(), contract).is_none(), "Duplicate schema declaration name");
    }
    let mut pending: BTreeSet<_> = registered.values().filter(|contract| contract.database || contract.wire).map(|contract| contract.name.clone()).collect();
    let mut visited = BTreeSet::new();
    while let Some(name) = pending.pop_first() {
        if !visited.insert(name.clone()) { continue; }
        let contract = registered.get(&name).ok_or_else(|| anyhow::anyhow!("Unregistered nested schema {name}"))?.clone();
        for field in &contract.fields { field.ty.references(&mut pending); }
        let key = contract.subject.as_deref().map(uuid::Uuid::parse_str).transpose()?
            .map(|subject| subject.to_string()).unwrap_or_else(|| contract.name.clone());
        ensure!(roots.insert(key, contract).is_none(), "Duplicate schema root identity");
    }
    Ok(roots)
}

impl DiscoveredContract {
    pub fn reconcile(&self, previous: Option<&Contract>) -> Result<Contract> {
        if let Some(previous) = previous { previous.validate()?; }
        let subject = self.subject.as_deref().map(uuid::Uuid::parse_str).transpose()?.map(|subject| subject.to_string());
        if let (Some(subject), Some(previous)) = (&subject, previous) {
            ensure!(subject == &previous.subject, "Schema type {} identity changed: expected UUID {}, supplied UUID {}", self.name, previous.subject, subject);
        }
        let mut contract = Contract {
            format_version: FORMAT_VERSION,
            subject: subject.or_else(|| previous.map(|value| value.subject.clone())).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            revision: previous.map(|value| value.revision).unwrap_or(0),
            fields: BTreeMap::new(),
            reserved_ids: previous.map(|value| value.reserved_ids.clone()).unwrap_or_default(),
            field_uuids: previous.map(|value| value.field_uuids.clone()).unwrap_or_default(),
        };
        let mut next_id = previous.into_iter().flat_map(|value| value.fields.keys().chain(&value.reserved_ids)).copied().max().unwrap_or(0);
        let mut names = BTreeSet::new();
        let mut uuids = BTreeSet::new();
        for field in &self.fields {
            ensure!(names.insert(&field.name), "Duplicate discovered field {}", field.name);
            let uuid = field.uuid.as_deref().map(uuid::Uuid::parse_str).transpose()?.map(|uuid| uuid.to_string());
            if let Some(uuid) = &uuid { ensure!(uuids.insert(uuid.clone()), "Duplicate field UUID"); }
            let old_name = field.rename_from.as_ref().unwrap_or(&field.name);
            let old = previous.and_then(|value| value.fields.values().find(|candidate| &candidate.name == old_name));
            let old = old.or_else(|| previous.and_then(|value| value.fields.values().find(|candidate| candidate.name == field.name)));
            ensure!(previous.is_none() || field.rename_from.is_none() || old.is_some(), "Unknown rename source {old_name}");
            let uuid_id = uuid.as_ref().and_then(|uuid| contract.field_uuids.get(uuid)).copied();
            if let Some(id) = uuid_id {
                ensure!(!contract.reserved_ids.contains(&id), "Removed field UUID cannot be reused");
                if field.rename_from.is_some() { ensure!(old.is_some_and(|old| old.id == id), "Field UUID and rename_from disagree"); }
            }
            let id = match (uuid_id, old) {
                (Some(id), _) => id,
                (None, Some(old)) => old.id,
                (None, None) => { next_id = next_id.checked_add(1).ok_or_else(|| anyhow::anyhow!("Field IDs exhausted"))?; next_id }
            };
            if let Some(uuid) = uuid {
                if let Some((expected, _)) = contract.field_uuids.iter().find(|(_, assigned)| **assigned == id) {
                    ensure!(expected == &uuid, "Schema field {}.{} identity changed: expected UUID {}, supplied UUID {}", self.name, field.name, expected, uuid);
                }
                contract.field_uuids.insert(uuid, id);
            }
            ensure!(contract.fields.insert(id, Field { id, name: field.name.clone(), ty: field.ty.clone(), required: field.required }).is_none(), "Ambiguous identity for {}", field.name);
        }
        if let Some(previous) = previous {
            for id in previous.fields.keys() {
                if !contract.fields.contains_key(id) { contract.reserved_ids.insert(*id); }
            }
            if contract.fields != previous.fields || contract.reserved_ids != previous.reserved_ids || contract.field_uuids != previous.field_uuids {
                contract.revision = previous.revision.checked_add(1).ok_or_else(|| anyhow::anyhow!("Schema revisions exhausted"))?;
            }
        }
        contract.validate()?;
        Ok(contract)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_uuid_lifecycle_preserves_keys_and_reserves_removals() {
        let identity = "693c4a43-d608-4a87-a79a-5629f0a1fa26";
        let mut descriptor = DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: None, database: true, wire: false,
            fields: vec![DiscoveredField { name: "name".into(), uuid: None, rename_from: None, ty: ValueType::String, required: true }],
        };
        let original = descriptor.reconcile(None).unwrap();
        assert!(!serde_json::to_value(&original).unwrap().as_object().unwrap().contains_key("field_uuids"));
        descriptor.fields[0].uuid = Some(identity.into());
        let assigned = descriptor.reconcile(Some(&original)).unwrap();
        assert_eq!(assigned.fields, original.fields);
        assert_eq!(assigned.field_uuids[identity], 1);
        descriptor.fields[0].name = "display_name".into();
        let renamed = descriptor.reconcile(Some(&assigned)).unwrap();
        assert_eq!(renamed.fields[&1].name, "display_name");
        descriptor.fields[0].uuid = None;
        assert_eq!(descriptor.reconcile(Some(&renamed)).unwrap(), renamed);
        descriptor.fields[0].uuid = Some(identity.into());
        assert_eq!(descriptor.reconcile(Some(&renamed)).unwrap(), renamed);
        let mut duplicate = descriptor.fields[0].clone();
        duplicate.name = "another_name".into();
        descriptor.fields.push(duplicate);
        assert!(descriptor.reconcile(Some(&renamed)).unwrap_err().to_string().contains("Duplicate field UUID"));
        descriptor.fields.clear();
        let removed = descriptor.reconcile(Some(&renamed)).unwrap();
        assert!(removed.reserved_ids.contains(&1));
        assert_eq!(removed.field_uuids[identity], 1);
        descriptor.fields.push(DiscoveredField { name: "replacement".into(), uuid: Some(identity.into()), rename_from: None, ty: ValueType::String, required: true });
        assert!(descriptor.reconcile(Some(&removed)).unwrap_err().to_string().contains("cannot be reused"));
    }

    #[test]
    fn identity_errors_report_expected_and_supplied_uuids() {
        let expected = "7997d059-e2ab-4db1-995f-2f0699c406d7";
        let supplied = "693c4a43-d608-4a87-a79a-5629f0a1fa26";
        let mut descriptor = DiscoveredContract {
            name: "Profile".into(), rename_from: None, subject: Some(expected.into()), database: true, wire: false,
            fields: vec![DiscoveredField { name: "name".into(), uuid: Some(expected.into()), rename_from: None, ty: ValueType::String, required: true }],
        };
        let previous = descriptor.reconcile(None).unwrap();
        descriptor.subject = Some(supplied.into());
        let error = descriptor.reconcile(Some(&previous)).unwrap_err().to_string();
        assert!(error.contains("Schema type Profile"));
        assert!(error.contains(&format!("expected UUID {expected}, supplied UUID {supplied}")));
        descriptor.subject = None;
        descriptor.fields[0].uuid = None;
        assert_eq!(descriptor.reconcile(Some(&previous)).unwrap(), previous);
        descriptor.fields[0].uuid = Some(supplied.into());
        let error = descriptor.reconcile(Some(&previous)).unwrap_err().to_string();
        assert!(error.contains("Schema field Profile.name"));
        assert!(error.contains(&format!("expected UUID {expected}, supplied UUID {supplied}")));
    }
}