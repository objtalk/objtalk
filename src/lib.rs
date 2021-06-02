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

pub const VERSION_STRING: &'static str = env!("VERSION_STRING");

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Object {
	pub name: String,
	pub value: Value,
	pub last_modified: DateTime<Utc>,
}
