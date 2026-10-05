use super::*;
mod service_graph;
pub use service_graph::{GraphCall, GraphServiceContracts, ServiceContractSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractRef {
    pub subject: String,
    pub revision: u32,
    pub fingerprint: String,
}

impl ContractRef {
    pub fn new(contract: &Contract) -> Result<Self> {
        Ok(Self { subject: contract.subject.clone(), revision: contract.revision, fingerprint: contract.fingerprint()? })
    }

    pub fn validate<'history>(&self, history: &'history History) -> Result<&'history Contract> {
        let contract = history.contracts.get(usize::try_from(self.revision)?).ok_or_else(|| anyhow::anyhow!("Unsupported wire revision"))?;
        ensure!(self.subject == contract.subject && self.fingerprint == contract.fingerprint()?, "Wire contract mismatch");
        Ok(contract)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireValue {
    pub contract: ContractRef,
    #[serde(deserialize_with = "unique_value")]
    pub value: Value,
}

pub(crate) fn unique_value<'de, Deserializer: serde::Deserializer<'de>>(deserializer: Deserializer) -> std::result::Result<Value, Deserializer::Error> {
    struct UniqueValue(Value);
    impl<'de> Deserialize<'de> for UniqueValue {
        fn deserialize<Deserializer: serde::Deserializer<'de>>(deserializer: Deserializer) -> std::result::Result<Self, Deserializer::Error> {
            unique_value(deserializer).map(Self)
        }
    }
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Value;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a JSON value without duplicate object keys")
        }
        fn visit_unit<Error: serde::de::Error>(self) -> std::result::Result<Value, Error> { Ok(Value::Null) }
        fn visit_bool<Error: serde::de::Error>(self, value: bool) -> std::result::Result<Value, Error> { Ok(Value::Bool(value)) }
        fn visit_i64<Error: serde::de::Error>(self, value: i64) -> std::result::Result<Value, Error> { Ok(value.into()) }
        fn visit_u64<Error: serde::de::Error>(self, value: u64) -> std::result::Result<Value, Error> { Ok(value.into()) }
        fn visit_f64<Error: serde::de::Error>(self, value: f64) -> std::result::Result<Value, Error> {
            serde_json::Number::from_f64(value).map(Value::Number).ok_or_else(|| Error::custom("Non-finite JSON number"))
        }
        fn visit_str<Error: serde::de::Error>(self, value: &str) -> std::result::Result<Value, Error> { Ok(value.into()) }
        fn visit_string<Error: serde::de::Error>(self, value: String) -> std::result::Result<Value, Error> { Ok(Value::String(value)) }
        fn visit_seq<Access: serde::de::SeqAccess<'de>>(self, mut access: Access) -> std::result::Result<Value, Access::Error> {
            let mut values = Vec::new();
            while let Some(UniqueValue(value)) = access.next_element()? { values.push(value); }
            Ok(Value::Array(values))
        }
        fn visit_map<Access: serde::de::MapAccess<'de>>(self, mut access: Access) -> std::result::Result<Value, Access::Error> {
            let mut values = Map::new();
            while let Some(key) = access.next_key::<String>()? {
                if values.contains_key(&key) { return Err(serde::de::Error::custom("Duplicate JSON object key")); }
                let UniqueValue(value) = access.next_value()?;
                values.insert(key, value);
            }
            Ok(Value::Object(values))
        }
    }
    deserializer.deserialize_any(Visitor)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCall {
    pub format_version: u16,
    pub request: WireValue,
    pub response: ContractRef,
    pub error: ContractRef,
}

#[derive(Clone, Debug)]
pub struct ServiceContracts {
    pub request: History,
    pub response: History,
    pub error: History,
}

fn latest(history: &History) -> &Contract {
    history.contracts.last().expect("validated contract history")
}

fn validate_reply_path(history: &History, reference: &ContractRef) -> Result<()> {
    reference.validate(history)?;
    for migration in history.migrations.iter().skip(usize::try_from(reference.revision)?) {
        let downgrade = migration.downgrade.as_ref().ok_or_else(|| anyhow::anyhow!("Unsupported wire downgrade"))?;
        ensure!(!downgrade.missing.values().any(|policy| matches!(policy, MissingPolicy::Unsupported)), "Unsupported historical reply value");
    }
    Ok(())
}

impl ServiceContracts {
    pub fn validate_types<Request: Schema, Response: Schema, Error: Schema>(&self) -> Result<()> {
        self.validate()?;
        for (descriptor, history) in [(Request::describe(), &self.request), (Response::describe(), &self.response), (Error::describe(), &self.error)] {
            ensure!(descriptor.reconcile(Some(latest(history)))? == *latest(history), "Rust wire type differs from its published contract");
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.request.validate()?;
        self.response.validate()?;
        self.error.validate()
    }

    pub fn call(&self, value: Value) -> Result<WireCall> {
        self.validate()?;
        latest(&self.request).validate_value(&value, ValueMode::Complete)?;
        Ok(WireCall {
            format_version: FORMAT_VERSION,
            request: WireValue { contract: ContractRef::new(latest(&self.request))?, value },
            response: ContractRef::new(latest(&self.response))?,
            error: ContractRef::new(latest(&self.error))?,
        })
    }

    pub fn decode_request(&self, call: &WireCall) -> Result<Value> {
        self.validate()?;
        ensure!(call.format_version == FORMAT_VERSION, "Unsupported wire contract format");
        call.request.contract.validate(&self.request)?;
        validate_reply_path(&self.response, &call.response)?;
        validate_reply_path(&self.error, &call.error)?;
        self.request.adapt(call.request.contract.revision, latest(&self.request).revision, call.request.value.clone(), ValueMode::Complete)
    }

    pub fn encode_reply(history: &History, reference: &ContractRef, value: Value) -> Result<WireValue> {
        history.validate()?;
        validate_reply_path(history, reference)?;
        let value = history.adapt(latest(history).revision, reference.revision, value, ValueMode::Complete)?;
        Ok(WireValue { contract: reference.clone(), value })
    }

    pub fn decode_reply(history: &History, expected: &ContractRef, reply: WireValue) -> Result<Value> {
        history.validate()?;
        ensure!(&reply.contract == expected, "Unexpected reply contract");
        reply.contract.validate(history)?.validate_value(&reply.value, ValueMode::Complete)?;
        Ok(reply.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_payload_rejects_duplicate_keys_at_every_depth() {
        let prefix = r#"{"contract":{"subject":"7997d059-e2ab-4db1-995f-2f0699c406d7","revision":0,"fingerprint":"fixture"},"value":"#;
        for payload in [r#"{"name":"first","name":"second"}"#, r#"{"items":[{"name":1,"name":2}]}"#] {
            assert!(serde_json::from_str::<WireValue>(&format!("{prefix}{payload}}}")).is_err());
        }
        let payload = r#"{"items":[null,true,-1,18446744073709551615,"text"]}"#;
        let decoded: WireValue = serde_json::from_str(&format!("{prefix}{payload}}}")).unwrap();
        assert_eq!(decoded.value, serde_json::from_str::<Value>(payload).unwrap());
    }

    #[test]
    fn wire_rejects_forged_contracts_and_unavailable_reply_paths() {
        let descriptor = DiscoveredContract { name: "request".into(), rename_from: None, subject: None, database: false, wire: true,
            fields: vec![DiscoveredField { name: "name".into(), uuid: None, rename_from: None, ty: ValueType::String, required: true }] };
        let baseline = History::new(descriptor.reconcile(None).unwrap()).unwrap();
        let client = ServiceContracts { request: baseline.clone(), response: baseline.clone(), error: baseline.clone() };
        let call = client.call(serde_json::json!({"name": "old"})).unwrap();
        let mut renamed = descriptor;
        renamed.fields[0].name = "display_name".into();
        renamed.fields[0].rename_from = Some("name".into());
        let target = renamed.reconcile(baseline.contracts.last()).unwrap();
        let upgrade = Adapter { from_fingerprint: baseline.contracts[0].fingerprint().unwrap(), to_fingerprint: target.fingerprint().unwrap(), missing: BTreeMap::new(), allow_drop: BTreeSet::new(), transforms: BTreeMap::new() };
        let downgrade = Adapter { from_fingerprint: upgrade.to_fingerprint.clone(), to_fingerprint: upgrade.from_fingerprint.clone(), missing: BTreeMap::new(), allow_drop: BTreeSet::new(), transforms: BTreeMap::new() };
        let mut history = baseline;
        history.append(target, Migration { id: "rename".into(), from_revision: 0, upgrade, downgrade: Some(downgrade) }).unwrap();
        let mut server = ServiceContracts { request: history.clone(), response: history.clone(), error: history };
        assert_eq!(server.decode_request(&call).unwrap(), serde_json::json!({"display_name": "old"}));
        let reply = ServiceContracts::encode_reply(&server.response, &call.response, serde_json::json!({"display_name": "new"})).unwrap();
        assert_eq!(ServiceContracts::decode_reply(&client.response, &call.response, reply).unwrap(), serde_json::json!({"name": "new"}));
        let mut forged = call.clone();
        forged.request.contract.fingerprint = "forged".into();
        assert!(server.decode_request(&forged).is_err());
        server.error.migrations[0].downgrade = None;
        assert!(server.decode_request(&call).is_err());
    }
}