//! JSON-RPC 2.0 protocol envelopes, request parsing, and error definitions.

use crate::json::JsonValue;

pub const ERROR_PARSE_ERROR: i32 = -32700;
pub const ERROR_INVALID_REQUEST: i32 = -32600;
pub const ERROR_METHOD_NOT_FOUND: i32 = -32601;
pub const ERROR_INVALID_PARAMS: i32 = -32602;
pub const ERROR_INTERNAL_ERROR: i32 = -32603;
pub const ERROR_SERVER_ERROR: i32 = -32000;

pub const MAX_BATCH_REQUESTS: usize = 100;

#[derive(Clone, PartialEq, Debug)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<JsonValue>,
    pub method: String,
    pub params: Vec<JsonValue>,
}

impl JsonRpcRequest {
    pub fn parse(payload: &str) -> Result<Self, JsonRpcError> {
        let json = JsonValue::parse(payload).map_err(|e| JsonRpcError::parse_error(&e))?;
        Self::from_json(&json)
    }

    pub fn from_json(val: &JsonValue) -> Result<Self, JsonRpcError> {
        let method = val
            .get("method")
            .and_then(|m| m.as_str())
            .ok_or_else(|| JsonRpcError::invalid_request("Missing 'method' field"))?
            .to_string();

        let id = val.get("id").cloned();
        let jsonrpc = val
            .get("jsonrpc")
            .and_then(|j| j.as_str())
            .unwrap_or("2.0")
            .to_string();

        let params = match val.get("params") {
            Some(JsonValue::Array(arr)) => arr.clone(),
            Some(val) => vec![val.clone()],
            None => Vec::new(),
        };

        Ok(Self {
            jsonrpc,
            id,
            method,
            params,
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<JsonValue>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: &str) -> Self {
        Self {
            code,
            message: message.to_string(),
            data: None,
        }
    }

    pub fn parse_error(msg: &str) -> Self {
        Self::new(ERROR_PARSE_ERROR, &format!("Parse error: {}", msg))
    }

    pub fn invalid_request(msg: &str) -> Self {
        Self::new(ERROR_INVALID_REQUEST, &format!("Invalid Request: {}", msg))
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(
            ERROR_METHOD_NOT_FOUND,
            &format!("Method not found: {}", method),
        )
    }

    pub fn invalid_params(msg: &str) -> Self {
        Self::new(ERROR_INVALID_PARAMS, &format!("Invalid params: {}", msg))
    }

    pub fn internal_error(msg: &str) -> Self {
        Self::new(ERROR_INTERNAL_ERROR, &format!("Internal error: {}", msg))
    }

    pub fn server_error(code: i32, msg: &str) -> Self {
        Self::new(code, msg)
    }

    pub fn revert(data_hex: &str) -> Self {
        Self {
            code: 3,
            message: "execution reverted".to_string(),
            data: Some(JsonValue::String(data_hex.to_string())),
        }
    }

    pub fn with_data(mut self, data: JsonValue) -> Self {
        self.data = Some(data);
        self
    }

    pub fn to_json(&self) -> JsonValue {
        let mut entries = vec![
            ("code".to_string(), JsonValue::Integer(self.code as i64)),
            (
                "message".to_string(),
                JsonValue::String(self.message.clone()),
            ),
        ];
        if let Some(ref data) = self.data {
            entries.push(("data".to_string(), data.clone()));
        }
        JsonValue::Object(entries)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<JsonValue>,
    pub result: Option<JsonValue>,
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<JsonValue>, result: JsonValue) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id.unwrap_or(JsonValue::Null)),
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<JsonValue>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id.unwrap_or(JsonValue::Null)),
            result: None,
            error: Some(error),
        }
    }

    pub fn to_json(&self) -> JsonValue {
        let mut entries = vec![
            (
                "jsonrpc".to_string(),
                JsonValue::String(self.jsonrpc.clone()),
            ),
            ("id".to_string(), self.id.clone().unwrap_or(JsonValue::Null)),
        ];

        if let Some(ref res) = self.result {
            entries.push(("result".to_string(), res.clone()));
        } else if let Some(ref err) = self.error {
            entries.push(("error".to_string(), err.to_json()));
        } else {
            entries.push(("result".to_string(), JsonValue::Null));
        }

        JsonValue::Object(entries)
    }

    pub fn serialize(&self) -> String {
        self.to_json().serialize()
    }
}
