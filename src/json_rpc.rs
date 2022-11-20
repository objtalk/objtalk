use crate::Object;
use serde::{Serialize,Deserialize};
use serde_json::Value;
use uuid::Uuid;

// { id, type: "get", name, value }
// { type: "response", requestId, result, error }

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum Request {
	Set {
		name: String,
		value: Value,
	},
	Patch {
		name: String,
		value: Value,
	},
	Get {
		pattern: String,
	},
	#[serde(rename_all = "camelCase")]
	Query {
		pattern: String,
		#[serde(default = "bool::default")]
		provide_rpc: bool,
	},
	#[serde(rename_all = "camelCase")]
	Unsubscribe {
		query_id: Uuid,
	},
	Remove {
		name: String,
	},
	Emit {
		object: String,
		event: String,
		data: Value,
	},
	Invoke {
		object: String,
		method: String,
		args: Value,
	},
	#[serde(rename = "invokeResult")]
	#[serde(rename_all = "camelCase")]
	InvokeResult {
		invocation_id: Uuid,
		result: Value,
	},
}

#[derive(Serialize, Debug)]
#[serde(untagged)]
pub enum Response {
	Success {
		success: bool,
	},
	Get {
		objects: Vec<Object>,
	},
	#[serde(rename_all = "camelCase")]
	Query {
		query_id: Uuid,
		objects: Vec<Object>,
	},
	Remove {
		existed: bool,
	}
}

#[derive(Deserialize, Debug)]
pub struct RequestMessage {
	pub id: Value,
	#[serde(flatten)]
	pub request: Request,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResponseMessage {
	pub request_id: Value,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub result: Option<Response>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub error: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum EventMessage {
	#[serde(rename_all = "camelCase")]
	QueryAdd {
		query_id: Uuid,
		object: Object,
	},
	#[serde(rename_all = "camelCase")]
	QueryChange {
		query_id: Uuid,
		object: Object,
	},
	#[serde(rename_all = "camelCase")]
	QueryRemove {
		query_id: Uuid,
		object: Object,
	},
	#[serde(rename_all = "camelCase")]
	QueryEvent {
		query_id: Uuid,
		object: String,
		event: String,
		data: Value,
	},
	#[serde(rename_all = "camelCase")]
	QueryInvocation {
		invocation_id: Uuid,
		query_id: Uuid,
		object: String,
		method: String,
		args: Value,
	},
	// TODO: not an event message?
	#[serde(rename_all = "camelCase")]
	InvocationResult {
		request_id: Value,
		#[serde(skip_serializing_if = "Option::is_none")]
		result: Option<Value>,
		#[serde(skip_serializing_if = "Option::is_none")]
		error: Option<String>,
	}
}
