use chrono::prelude::*;
use crate::{Object, VERSION_STRING};
use crate::patterns::Pattern;
use crate::server::logger::{Logger, LogMessage};
use crate::server::storage::Storage;
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver, TryRecvError};
use futures::StreamExt;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

pub mod storage;
pub mod json_rpc;
pub mod http_transport;
pub mod tcp_transport;
pub mod config;
pub mod logger;
pub mod admin;

#[derive(Error, Debug, PartialEq)]
pub enum Error {
	#[error("invalid object name")]
	InvalidObjectName,
	#[error("object not found")]
	ObjectNotFound,
	#[error("object values not mergeable")]
	CantMergeObjects,
	#[error("query not found")]
	QueryNotFound,
	#[error("client not found")]
	ClientNotFound,
}

fn validate_object_name(name: &str) -> Result<(), Error> {
	if name == "" || name.starts_with("$") {
		Err(Error::InvalidObjectName)
	} else {
		Ok(())
	}
}

fn merge_into_object(old: &mut Value, new: &Value) -> Result<(), Error> {
	match (old, new) {
		(&mut Value::Object(ref mut a), &Value::Object(ref b)) => {
			for (k, v) in b {
				a.insert(k.to_string(), v.clone());
			}
			
			Ok(())
		},
		_ => Err(Error::CantMergeObjects),
	}
}

#[derive(Debug)]
pub enum Message {
	QueryAdd {
		query_id: Uuid,
		object: Object,
	},
	QueryChange {
		query_id: Uuid,
		object: Object,
	},
	QueryRemove {
		query_id: Uuid,
		object: Object,
	},
	QueryEvent {
		query_id: Uuid,
		object: String,
		event: String,
		data: Value,
	}
}

#[derive(Debug)]
struct Query {
	id: Uuid,
	pattern: Pattern,
	objects: HashSet<String>,
}

#[derive(Debug)]
pub struct ClientState {
	id: Uuid,
	queries: Vec<Query>,
	inbox_tx: UnboundedSender<Message>,
}

pub struct Client {
	id: Uuid,
	server: Server,
	inbox_rx: UnboundedReceiver<Message>,
}

impl Client {
	pub async fn inbox_next(&mut self) -> Option<Message> {
		self.inbox_rx.next().await
	}
	
	pub fn inbox_try_next(&mut self) -> Result<Option<Message>, TryRecvError> {
		self.inbox_rx.try_next()
	}
}

impl Drop for Client {
	fn drop(&mut self) {
		self.server.client_disconnect(self.id);
	}
}

#[derive(Clone)]
pub struct Server {
	shared: Arc<Shared>,
}

struct Shared {
	state: Mutex<State>,
}

struct State {
	objects: HashMap<String,Object>,
	clients: HashMap<Uuid,ClientState>,
	storage: Option<Box<dyn Storage + Send>>,
	logger: Box<dyn Logger + Send>,
}

impl State {
	fn emit(&mut self, object: String, event: String, data: Value) -> Result<(), Error> {
		if self.objects.get(&object).is_none() {
			return Err(Error::ObjectNotFound)
		}
		
		for client in self.clients.values_mut() {
			for query in &mut client.queries {
				if query.objects.contains(&object) {
					let msg = Message::QueryEvent {
						query_id: query.id,
						object: object.to_string(),
						event: event.to_string(),
						data: data.clone(),
					};
					let _ = client.inbox_tx.unbounded_send(msg);
				}
			}
		}
		
		Ok(())
	}
	
	fn log(&mut self, message: LogMessage) {
		self.logger.log(&message);
		
		self.emit("$system".to_string(), "log".to_string(), serde_json::to_value(message).unwrap()).unwrap()
	}
}

impl Server {
	pub fn new(storage: Option<Box<dyn Storage + Send>>, logger: Box<dyn Logger + Send>) -> Self {
		let mut objects = HashMap::new();
		
		objects.insert("$system".to_string(), Object {
			name: "$system".to_string(),
			value: json!({ "version": VERSION_STRING }),
			last_modified: Utc::now(),
		});
		
		if let Some(ref storage) = storage {
			for object in storage.get_objects() {
				objects.insert(object.name.clone(), object);
			}
		}
		
		let shared = Arc::new(Shared {
			state: Mutex::new(State {
				objects,
				clients: HashMap::new(),
				storage,
				logger,
			})
		});
		
		Server { shared }
	}
	
	pub fn client_connect(&self) -> Client {
		let mut state = self.shared.state.lock().unwrap();
		
		let id = Uuid::new_v4();
		
		let (tx, rx) = unbounded();
		
		let client = ClientState {
			id,
			queries: vec![],
			inbox_tx: tx,
		};
		
		state.log(LogMessage::ClientConnect { client: id });
		
		state.clients.insert(id, client);
		
		Client { id, server: self.clone(), inbox_rx: rx }
	}
	
	fn client_disconnect(&self, client_id: Uuid) {
		let mut state = self.shared.state.lock().unwrap();
		
		state.log(LogMessage::ClientDisconnect { client: client_id });
		
		state.clients.remove(&client_id);
	}
	
	pub fn set(&self, name: &str, value: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		let inserted: bool;
		
		validate_object_name(name)?;
		
		state.log(LogMessage::Set { object: name.to_string(), value: value.clone(), client: client.id });
		
		if let Some(object) = state.objects.get_mut(name) {
			object.value = value;
			object.last_modified = Utc::now();
			inserted = false;
		} else {
			state.objects.insert(name.to_string(), Object {
				name: name.to_string(),
				value,
				last_modified: Utc::now(),
			});
			inserted = true;
		}
		
		let object = state.objects[name].clone();
		
		if let Some(storage) = &state.storage {
			if inserted {
				storage.add_object(object.clone());
			} else {
				storage.change_object(object.clone());
			}
		}
		
		for client in state.clients.values_mut() {
			for query in &mut client.queries {
				if query.pattern.matches_str(name) {
					let msg = if query.objects.contains(name) {
						Message::QueryChange {
							query_id: query.id,
							object: object.clone(),
						}
					} else {
						query.objects.insert(name.to_string());
						Message::QueryAdd {
							query_id: query.id,
							object: object.clone(),
						}
					};
					
					let _ = client.inbox_tx.unbounded_send(msg);
				}
			}
		}
		
		Ok(())
	}
	
	pub fn patch(&self, name: &str, value: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		let inserted: bool;
		
		validate_object_name(name)?;
		
		if !value.is_object() {
			return Err(Error::CantMergeObjects);
		}
		
		state.log(LogMessage::Patch { object: name.to_string(), value: value.clone(), client: client.id });
		
		if let Some(object) = state.objects.get_mut(name) {
			merge_into_object(&mut object.value, &value)?;
			object.last_modified = Utc::now();
			inserted = false;
		} else {
			state.objects.insert(name.to_string(), Object {
				name: name.to_string(),
				value,
				last_modified: Utc::now(),
			});
			inserted = true;
		}
		
		let object = state.objects[name].clone();
		
		if let Some(storage) = &state.storage {
			if inserted {
				storage.add_object(object.clone());
			} else {
				storage.change_object(object.clone());
			}
		}
		
		for client in state.clients.values_mut() {
			for query in &mut client.queries {
				if query.pattern.matches_str(name) {
					let msg = if query.objects.contains(name) {
						Message::QueryChange {
							query_id: query.id,
							object: object.clone(),
						}
					} else {
						query.objects.insert(name.to_string());
						Message::QueryAdd {
							query_id: query.id,
							object: object.clone(),
						}
					};
					
					let _ = client.inbox_tx.unbounded_send(msg);
				}
			}
		}
		
		Ok(())
	}
	
	pub fn get(&self, pattern: &Pattern, client: &Client) -> Vec<Object> {
		let mut state = self.shared.state.lock().unwrap();
		
		state.log(LogMessage::Get { pattern: pattern.string.clone(), client: client.id });
		
		state.objects.values().filter(|object| {
			pattern.matches(&object.name)
		}).cloned().collect()
	}
	
	pub fn query(&self, pattern: &Pattern, client: &Client) -> Result<(Uuid, Vec<Object>),Error> {
		let mut state = self.shared.state.lock().unwrap();
		
		let id = Uuid::new_v4();
		
		state.log(LogMessage::Query { pattern: pattern.string.clone(), query: id, client: client.id });
		
		let objects: Vec<Object> = state.objects.values().filter(|object| {
			pattern.matches(&object.name)
		}).cloned().collect();
		
		if let Some(client) = state.clients.get_mut(&client.id) {
			client.queries.push(Query {
				id,
				pattern: pattern.clone(),
				objects: HashSet::from_iter(objects.iter().map(|object| object.name.clone())),
			});
			Ok((id, objects))
		} else {
			Err(Error::ClientNotFound)
		}
	}
	
	pub fn unsubscribe(&self, query_id: Uuid, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		
		state.log(LogMessage::Unsubscribe { query: query_id, client: client.id });
		
		let client = state.clients.get_mut(&client.id).unwrap();	
		
		if let Some(index) = client.queries.iter().position(|query| query.id == query_id) {
			client.queries.remove(index);
				
			Ok(())
		} else {
			Err(Error::QueryNotFound)
		}
	}
	
	pub fn remove(&self, name: String, client: &Client) -> bool {
		let mut state = self.shared.state.lock().unwrap();
		
		if validate_object_name(&name).is_err() {
			return false;
		}
		
		if let Some(object) = state.objects.remove(&name) {
			state.log(LogMessage::Remove { object: name.clone(), client: client.id });
			
			if let Some(storage) = &state.storage {
				storage.remove_object(object.clone());
			}
			
			for client in state.clients.values_mut() {
				for query in &mut client.queries {
					if query.objects.contains(&name) {
						let msg = Message::QueryRemove {
							query_id: query.id,
							object: object.clone()
						};
						let _ = client.inbox_tx.unbounded_send(msg);
					}
				}
			}
			
			return true;
		}
		
		return false;
	}
	
	pub fn emit(&self, object: String, event: String, data: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		
		validate_object_name(&object)?;
		
		state.log(LogMessage::Emit { object: object.clone(), event: event.clone(), data: data.clone(), client: client.id });
		
		state.emit(object, event, data)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::server::logger::NullLogger;
	use serde_json::json;
	
	fn create_server() -> Server {
		Server::new(None, Box::new(NullLogger))
	}
	
	#[test]
	fn test_set_insert() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": true }), &client).unwrap();
		
		let state = server.shared.state.lock().unwrap();
		assert!(state.objects.contains_key("foo"));
		assert_eq!(state.objects["foo"].name, "foo");
		assert_eq!(state.objects["foo"].value, json!({ "bar": true }));
	}
	
	#[test]
	fn test_set_update() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": true }), &client).unwrap();
		server.set("foo", json!({ "bar": false }), &client).unwrap();
		
		let state = server.shared.state.lock().unwrap();
		assert_eq!(state.objects["foo"].value, json!({ "bar": false }));
	}
	
	#[test]
	fn test_set_invalid_name() {
		let server = create_server();
		let client = server.client_connect();
		
		let result = server.set("$system", json!({ "bar": true }), &client);
		assert_eq!(result, Err(Error::InvalidObjectName));
	}
	
	#[test]
	fn test_patch_invalid_name() {
		let server = create_server();
		let client = server.client_connect();
		
		let result = server.patch("$system", json!({ "bar": true }), &client);
		assert_eq!(result, Err(Error::InvalidObjectName));
	}
	
	#[test]
	fn test_patch_insert() {
		let server = create_server();
		let client = server.client_connect();
		
		server.patch("foo", json!({ "bar": true }), &client).unwrap();
		
		let state = server.shared.state.lock().unwrap();
		assert!(state.objects.contains_key("foo"));
		assert_eq!(state.objects["foo"].name, "foo");
		assert_eq!(state.objects["foo"].value, json!({ "bar": true }));
	}
	
	#[test]
	fn test_patch_insert_non_object() {
		let server = create_server();
		let client = server.client_connect();
		
		let result = server.patch("foo", json!(42), &client);
		assert_eq!(result, Err(Error::CantMergeObjects));
	}
	
	#[test]
	fn test_patch_update_non_object() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!(42), &client).unwrap();
		
		let result = server.patch("foo", json!({ "baz": true }), &client);
		assert_eq!(result, Err(Error::CantMergeObjects));
	}
	
	#[test]
	fn test_patch_update_with_non_object() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": true }), &client).unwrap();
		
		let result = server.patch("foo", json!(42), &client);
		assert_eq!(result, Err(Error::CantMergeObjects));
	}
	
	#[test]
	fn test_patch_update() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": true }), &client).unwrap();
		server.patch("foo", json!({ "baz": true }), &client).unwrap();
		
		let state = server.shared.state.lock().unwrap();
		assert!(state.objects.contains_key("foo"));
		assert_eq!(state.objects["foo"].name, "foo");
		assert_eq!(state.objects["foo"].value, json!({ "bar": true, "baz": true }));
	}
	
	#[test]
	fn test_patch_update_non_deep() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "on": true, "color": { "hue": 100, "saturation": 100 } }), &client).unwrap();
		server.patch("foo", json!({ "color": { "temp": 50 } }), &client).unwrap();
		
		let state = server.shared.state.lock().unwrap();
		assert!(state.objects.contains_key("foo"));
		assert_eq!(state.objects["foo"].name, "foo");
		assert_eq!(state.objects["foo"].value, json!({ "on": true, "color": { "temp": 50 } }));
	}
	
	#[test]
	fn test_get() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("livingroom/temperature", json!({ "temp": 20.3 }), &client).unwrap();
		server.set("livingroom/humidity", json!({ "humid": 40 }), &client).unwrap();
		server.set("bedroom/temperature", json!({ "temp": 19 }), &client).unwrap();
		
		let result = server.get(&Pattern::compile("$system").unwrap(), &client);
		assert_eq!(result.len(), 1);
		
		let result = server.get(&Pattern::compile("*").unwrap(), &client);
		assert_eq!(result.len(), 3);
		
		let result = server.get(&Pattern::compile("*,$system").unwrap(), &client);
		assert_eq!(result.len(), 4);
		
		let result = server.get(&Pattern::compile("+/temperature,+/humidity").unwrap(), &client);
		assert_eq!(result.len(), 3);
		
		let result = server.get(&Pattern::compile("livingroom/+").unwrap(), &client);
		assert_eq!(result.len(), 2);
		
		let result = server.get(&Pattern::compile("+/humidity").unwrap(), &client);
		assert_eq!(result.len(), 1);
	}
	
	#[test]
	fn test_query() {
		let server = create_server();
		let client1 = server.client_connect();
		let mut client2 = server.client_connect();
		
		server.set("livingroom/temperature", json!({ "temp": 20.3 }), &client1).unwrap();
		
		let (query_id, objects) = server.query(&Pattern::compile("+/temperature").unwrap(), &client2).unwrap();
		
		assert_eq!(objects.len(), 1);
		assert_eq!(objects[0].name, "livingroom/temperature");
		assert_eq!(objects[0].value, json!({ "temp": 20.3 }));
		
		server.set("livingroom/temperature", json!({ "temp": 20.4 }), &client1).unwrap();
		server.set("livingroom/temperature", json!({ "temp": 20.5 }), &client1).unwrap();
		server.set("bedroom/temperature", json!({ "temp": 19.0 }), &client1).unwrap();
		server.set("bedroom/temperature", json!({ "temp": 19.1 }), &client1).unwrap();
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryChange { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "livingroom/temperature");
			assert_eq!(object.value, json!({ "temp": 20.4 }));
		} else {
			assert!(false);
		}
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryChange { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "livingroom/temperature");
			assert_eq!(object.value, json!({ "temp": 20.5 }));
		} else {
			assert!(false);
		}
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryAdd { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "bedroom/temperature");
			assert_eq!(object.value, json!({ "temp": 19.0 }));
		} else {
			assert!(false);
		}
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryChange { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "bedroom/temperature");
			assert_eq!(object.value, json!({ "temp": 19.1 }));
		} else {
			assert!(false);
		}
		
		assert!(client2.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_unsubscribe() {
		let server = create_server();
		let client1 = server.client_connect();
		let mut client2 = server.client_connect();
		
		server.set("livingroom/temperature", json!({ "temp": 20.3 }), &client1).unwrap();
		
		let (query_id, _) = server.query(&Pattern::compile("+/temperature").unwrap(), &client2).unwrap();
		
		server.set("livingroom/temperature", json!({ "temp": 20.4 }), &client1).unwrap();
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		if let Message::QueryChange { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "livingroom/temperature");
			assert_eq!(object.value, json!({ "temp": 20.4 }));
		} else {
			assert!(false);
		}
		
		server.unsubscribe(query_id, &client2).unwrap();
		
		server.set("livingroom/temperature", json!({ "temp": 20.5 }), &client1).unwrap();
		
		assert!(client2.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_remove_non_existing() {
		let server = create_server();
		let client = server.client_connect();
		
		let existed = server.remove("foo".to_string(), &client);
		assert!(!existed);
	}
	
	#[test]
	fn test_remove_existing() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": 1 }), &client).unwrap();
		
		let existed = server.remove("foo".to_string(), &client);
		assert!(existed);
	}
	
	#[test]
	fn test_remove_query() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": 1 }), &client).unwrap();
		
		let mut client = server.client_connect();
		
		let (query_id, _) = server.query(&Pattern::compile("*").unwrap(), &client).unwrap();
		
		server.remove("foo".to_string(), &client);
		
		let msg = client.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryRemove { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "foo");
			assert_eq!(object.value, json!({ "bar": 1 }));
		} else {
			assert!(false);
		}
		
		assert!(client.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_emit_event() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("gamepad", json!({ "buttons": ["a", "b"] }), &client).unwrap();
		
		let mut client = server.client_connect();
		
		let (query_id, _) = server.query(&Pattern::compile("*").unwrap(), &client).unwrap();
		
		server.emit("gamepad".to_string(), "buttonpress".to_string(), json!({ "button": "a" }), &client).unwrap();
		
		let msg = client.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryEvent { query_id: msg_query_id, object, event, data } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object, "gamepad");
			assert_eq!(event, "buttonpress");
			assert_eq!(data, json!({ "button": "a" }));
		} else {
			assert!(false);
		}
		
		assert!(client.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_emit_event_doesnt_exist() {
		let server = create_server();
		let client = server.client_connect();
		
		let result = server.emit("gamepad".to_string(), "buttonpress".to_string(), json!({ "button": "a" }), &client);
		
		assert_eq!(result, Err(Error::ObjectNotFound));
	}
}
