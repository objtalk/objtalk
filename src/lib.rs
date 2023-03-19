#[cfg(feature = "server")]
pub mod patterns;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "client")]
pub mod client;
pub mod json_rpc;

use serde::{Serialize, Deserialize};
use serde_json::Value;
use chrono::prelude::*;
use uuid::Uuid;

pub const VERSION_STRING: &'static str = env!("VERSION_STRING");

#[derive(Serialize, Debug, Eq, Hash, PartialEq, Clone, Copy)]
pub struct StreamId(Uuid);

impl StreamId {
	pub fn new() -> StreamId {
		StreamId(Uuid::new_v4())
	}
}

#[derive(Serialize, Debug, Eq, Hash, PartialEq, Clone, Copy)]
pub struct ClientStreamIndex(u32);

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Object {
	pub name: String,
	pub value: Value,
	pub last_modified: DateTime<Utc>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum Command {
	Set {
		name: String,
		value: Value,
	},
	Patch {
		name: String,
		value: Value,
	},
	Remove {
		name: String,
	},
	Emit {
		object: String,
		event: String,
		data: Value,
	},
}
