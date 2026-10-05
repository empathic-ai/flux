use super::*;
use serde::de::DeserializeOwned;

pub trait ServiceContractSet: Clone + Send + Sync + 'static {
    type Call: Clone + Serialize + DeserializeOwned + Send + Sync + 'static;
    type Reply: Serialize + DeserializeOwned + Send + 'static;
    const OPERATION_SUFFIX: &'static str;

    fn validate_types<Request: Schema, Response: Schema, Error: Schema>(&self) -> Result<()>;
    fn call(&self, value: Value) -> Result<Self::Call>;
    fn decode_request(&self, call: &Self::Call) -> Result<Value>;
    fn encode_response(&self, call: &Self::Call, value: Value) -> Result<Self::Reply>;
    fn encode_error(&self, call: &Self::Call, value: Value) -> Result<Self::Reply>;
    fn decode_response(&self, call: &Self::Call, reply: Self::Reply) -> Result<Value>;
    fn decode_error(&self, call: &Self::Call, reply: Self::Reply) -> Result<Value>;
}

impl ServiceContractSet for ServiceContracts {
    type Call = WireCall;
    type Reply = WireValue;
    const OPERATION_SUFFIX: &'static str = ".schema";

    fn validate_types<Request: Schema, Response: Schema, Error: Schema>(&self) -> Result<()> {
        self.validate_types::<Request, Response, Error>()
    }
    fn call(&self, value: Value) -> Result<WireCall> { self.call(value) }
    fn decode_request(&self, call: &WireCall) -> Result<Value> { self.decode_request(call) }
    fn encode_response(&self, call: &WireCall, value: Value) -> Result<WireValue> {
        Self::encode_reply(&self.response, &call.response, value)
    }
    fn encode_error(&self, call: &WireCall, value: Value) -> Result<WireValue> {
        Self::encode_reply(&self.error, &call.error, value)
    }
    fn decode_response(&self, call: &WireCall, reply: WireValue) -> Result<Value> {
        Self::decode_reply(&self.response, &call.response, reply)
    }
    fn decode_error(&self, call: &WireCall, reply: WireValue) -> Result<Value> {
        Self::decode_reply(&self.error, &call.error, reply)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphCall {
    pub format_version: u16,
    pub request: GraphValue,
    pub response: GraphRef,
    pub error: GraphRef,
}

#[derive(Clone, Debug)]
pub struct GraphServiceContracts {
    catalog: std::sync::Arc<Catalog>,
    request: GraphRef,
    response: GraphRef,
    error: GraphRef,
}

impl GraphServiceContracts {
    pub fn new<Request: Schema, Response: Schema, Error: Schema>(catalog: Catalog) -> Result<Self> {
        let request = catalog.graph_for::<Request>()?;
        let response = catalog.graph_for::<Response>()?;
        let error = catalog.graph_for::<Error>()?;
        Ok(Self { catalog: std::sync::Arc::new(catalog), request, response, error })
    }

    fn reply_path(&self, source: &GraphRef, target: &GraphRef) -> Result<()> {
        ensure!(source.root == target.root, "Unexpected reply subject");
        target.validate(&self.catalog)?;
        for (subject, reference) in &target.contracts {
            let current = source.contracts.get(subject).ok_or_else(|| anyhow::anyhow!("Unsupported reply graph subject"))?;
            ensure!(reference.revision <= current.revision, "Unsupported future reply revision");
            let history = self.catalog.subject_history(subject)?;
            for migration in &history.migrations[reference.revision as usize..current.revision as usize] {
                let downgrade = migration.downgrade.as_ref().ok_or_else(|| anyhow::anyhow!("Unsupported graph reply downgrade"))?;
                ensure!(!downgrade.missing.values().any(|policy| matches!(policy, MissingPolicy::Unsupported)), "Unsupported historical graph reply value");
            }
        }
        Ok(())
    }

    fn encode_reply(&self, source: &GraphRef, target: &GraphRef, value: Value) -> Result<GraphValue> {
        self.reply_path(source, target)?;
        let value = source.adapt(&self.catalog, target, value, ValueMode::Complete)?;
        let reply = GraphValue { format_version: FORMAT_VERSION, graph: target.clone(), value };
        reply.to_json(&self.catalog, ValueMode::Complete)?;
        Ok(reply)
    }

    fn decode_reply(&self, expected: &GraphRef, reply: GraphValue) -> Result<Value> {
        ensure!(&reply.graph == expected, "Unexpected reply graph");
        ensure!(serde_json::to_vec(&reply)?.len() <= MAX_GRAPH_BYTES, "Graph reply exceeds byte limit");
        reply.decode(&self.catalog, expected, ValueMode::Complete)
    }
}

impl ServiceContractSet for GraphServiceContracts {
    type Call = GraphCall;
    type Reply = GraphValue;
    const OPERATION_SUFFIX: &'static str = ".graph";

    fn validate_types<Request: Schema, Response: Schema, Error: Schema>(&self) -> Result<()> {
        ensure!(self.request == self.catalog.graph_for::<Request>()?, "Unexpected request graph");
        ensure!(self.response == self.catalog.graph_for::<Response>()?, "Unexpected response graph");
        ensure!(self.error == self.catalog.graph_for::<Error>()?, "Unexpected error graph");
        Ok(())
    }

    fn call(&self, value: Value) -> Result<GraphCall> {
        self.request.validate_value(&self.catalog, &value, ValueMode::Complete)?;
        let call = GraphCall { format_version: FORMAT_VERSION,
            request: GraphValue { format_version: FORMAT_VERSION, graph: self.request.clone(), value },
            response: self.response.clone(), error: self.error.clone() };
        ensure!(serde_json::to_vec(&call)?.len() <= MAX_GRAPH_BYTES, "Graph call exceeds byte limit");
        Ok(call)
    }

    fn decode_request(&self, call: &GraphCall) -> Result<Value> {
        ensure!(call.format_version == FORMAT_VERSION, "Unsupported graph call format");
        ensure!(serde_json::to_vec(call)?.len() <= MAX_GRAPH_BYTES, "Graph call exceeds byte limit");
        self.reply_path(&self.response, &call.response)?;
        self.reply_path(&self.error, &call.error)?;
        call.request.clone().decode(&self.catalog, &self.request, ValueMode::Complete)
    }

    fn encode_response(&self, call: &GraphCall, value: Value) -> Result<GraphValue> {
        self.encode_reply(&self.response, &call.response, value)
    }
    fn encode_error(&self, call: &GraphCall, value: Value) -> Result<GraphValue> {
        self.encode_reply(&self.error, &call.error, value)
    }
    fn decode_response(&self, call: &GraphCall, reply: GraphValue) -> Result<Value> {
        self.decode_reply(&call.response, reply)
    }
    fn decode_error(&self, call: &GraphCall, reply: GraphValue) -> Result<Value> {
        self.decode_reply(&call.error, reply)
    }
}