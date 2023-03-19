use chrono::prelude::*;
use crate::{Object, Command, StreamId, ClientStreamIndex, VERSION_STRING};
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
	#[error("not invocable")]
	ObjectNotInvocable,
	#[error("invocation not found")]
	InvocationNotFound,
	#[error("stream not found")]
	StreamNotFound,
	#[error("stream already open")]
	StreamAlreadyOpen,
	#[error("stream not open")]
	StreamNotOpen,
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
	},
	QueryInvocation {
		query_id: Uuid,
		invocation_id: Uuid,
		object: String,
		method: String,
		args: Value,
	},
	InvocationResult {
		request_id: Value,
		result: Result<Value, Error>,
	},
	StreamOpen {
		index: ClientStreamIndex,
	},
	StreamClosed {
		index: ClientStreamIndex,
	},
	StreamData {
		index: ClientStreamIndex,
		data: Vec<u8>,
	},
}

#[derive(Debug, Clone)]
struct Invocation {
	id: Uuid,
	client_id: Uuid,
	request_id: Value,
	query_id: Uuid,
}

#[derive(Debug)]
struct Query {
	id: Uuid,
	pattern: Pattern,
	provide_rpc: bool,
	objects: HashSet<String>,
}

#[derive(Debug)]
pub struct ClientState {
	#[allow(dead_code)]
	id: Uuid,
	queries: Vec<Query>,
	invocations: Vec<Invocation>,
	inbox_tx: UnboundedSender<Message>,
	disconnect_commands: Vec<Command>,
	next_stream_index: u32,
	streams: HashMap<ClientStreamIndex, StreamId>,
}

#[derive(Debug)]
pub struct StreamClient {
	client_id: Uuid,
	stream_index: ClientStreamIndex,
}

#[derive(Debug)]
pub struct StreamState {
	#[allow(dead_code)]
	id: StreamId,
	client_a: StreamClient,
	client_b: Option<StreamClient>,
}

impl StreamState {
	pub fn get_other(&self, client_id: &Uuid, stream_index: &ClientStreamIndex) -> Option<&StreamClient> {
		if &self.client_a.client_id == client_id && &self.client_a.stream_index == stream_index {
			return self.client_b.as_ref();
		}
		
		if let Some(client_b) = &self.client_b {
			if &client_b.client_id == client_id && &client_b.stream_index == stream_index {
				return Some(&self.client_a);
			}
		}
		
		None
	}
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
	streams: HashMap<StreamId,StreamState>,
}

impl State {
	fn set(&mut self, name: &str, value: Value, client_id: Uuid) -> Result<(), Error> {
		let inserted: bool;
		
		validate_object_name(name)?;
		
		self.log(LogMessage::Set { object: name.to_string(), value: value.clone(), client: client_id });
		
		if let Some(object) = self.objects.get_mut(name) {
			object.value = value;
			object.last_modified = Utc::now();
			inserted = false;
		} else {
			self.objects.insert(name.to_string(), Object {
				name: name.to_string(),
				value,
				last_modified: Utc::now(),
			});
			inserted = true;
		}
		
		let object = self.objects[name].clone();
		
		if let Some(storage) = &self.storage {
			if inserted {
				storage.add_object(object.clone());
			} else {
				storage.change_object(object.clone());
			}
		}
		
		for client in self.clients.values_mut() {
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
	
	fn patch(&mut self, name: &str, value: Value, client_id: Uuid) -> Result<(), Error> {
		let inserted: bool;
		
		validate_object_name(name)?;
		
		if !value.is_object() {
			return Err(Error::CantMergeObjects);
		}
		
		self.log(LogMessage::Patch { object: name.to_string(), value: value.clone(), client: client_id });
		
		if let Some(object) = self.objects.get_mut(name) {
			merge_into_object(&mut object.value, &value)?;
			object.last_modified = Utc::now();
			inserted = false;
		} else {
			self.objects.insert(name.to_string(), Object {
				name: name.to_string(),
				value,
				last_modified: Utc::now(),
			});
			inserted = true;
		}
		
		let object = self.objects[name].clone();
		
		if let Some(storage) = &self.storage {
			if inserted {
				storage.add_object(object.clone());
			} else {
				storage.change_object(object.clone());
			}
		}
		
		for client in self.clients.values_mut() {
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
	
	fn remove(&mut self, name: &str, client_id: Uuid) -> Result<bool, Error> {
		validate_object_name(name)?;
		
		if let Some(object) = self.objects.remove(name) {
			self.log(LogMessage::Remove { object: name.to_string(), client: client_id });
			
			if let Some(storage) = &self.storage {
				storage.remove_object(object.clone());
			}
			
			for client in self.clients.values_mut() {
				for query in &mut client.queries {
					if query.objects.contains(name) {
						let msg = Message::QueryRemove {
							query_id: query.id,
							object: object.clone()
						};
						let _ = client.inbox_tx.unbounded_send(msg);
						
						query.objects.remove(name);
					}
				}
			}
			
			Ok(true)
		} else {
			Ok(false)
		}
	}
	
	fn internal_emit(&mut self, object: &str, event: &str, data: Value) -> Result<(), Error> {
		if self.objects.get(object).is_none() {
			return Err(Error::ObjectNotFound)
		}
		
		for client in self.clients.values_mut() {
			for query in &mut client.queries {
				if query.objects.contains(object) {
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
	
	fn emit(&mut self, object: &str, event: &str, data: Value, client_id: Uuid) -> Result<(), Error> {
		validate_object_name(object)?;
		
		self.log(LogMessage::Emit { object: object.to_string(), event: event.to_string(), data: data.clone(), client: client_id });
		self.internal_emit(object, event, data)
	}
	
	fn invoke(&mut self, object: &str, method: &str, args: Value, request_id: Value, client_id: Uuid) -> Result<(), Error> {
		validate_object_name(object)?;
		
		let invocation_id = Uuid::new_v4();
		
		self.log(LogMessage::Invoke { object: object.to_string(), method: method.to_string(), args: args.clone(), invocation_id: invocation_id.clone(), client: client_id });
		
		if self.objects.get(object).is_none() {
			return Err(Error::ObjectNotFound)
		}
		
		for responder in self.clients.values_mut() {
			for query in &mut responder.queries {
				if query.provide_rpc {
					if query.objects.contains(object) {
						responder.invocations.push(Invocation {
							id: invocation_id,
							client_id,
							request_id,
							query_id: query.id,
						});
						
						let msg = Message::QueryInvocation {
							query_id: query.id,
							invocation_id,
							object: object.to_string(),
							method: method.to_string(),
							args: args.clone(),
						};
						let _ = responder.inbox_tx.unbounded_send(msg);
						
						return Ok(())
					}
				}
			}
		}
		
		Err(Error::ObjectNotInvocable)
	}
	
	fn create_stream(&mut self, client_id: Uuid) -> Result<(StreamId, ClientStreamIndex), Error> {
		let client = self.clients.get_mut(&client_id).ok_or(Error::ClientNotFound)?;
		
		let stream_id = StreamId::new();
		let stream_index = ClientStreamIndex(client.next_stream_index);
		client.next_stream_index += 1;
		
		self.streams.insert(stream_id, StreamState {
			id: stream_id,
			client_a: StreamClient { client_id, stream_index },
			client_b: None,
		});
		
		client.streams.insert(stream_index, stream_id);
		
		self.log(LogMessage::StreamCreate { id: stream_id, index: stream_index, client: client_id });
		
		Ok((stream_id, stream_index))
	}
	
	fn open_stream(&mut self, id: StreamId, client_id: Uuid) -> Result<ClientStreamIndex, Error> {
		let client = self.clients.get_mut(&client_id).ok_or(Error::ClientNotFound)?;
		let stream = self.streams.get_mut(&id).ok_or(Error::StreamNotFound)?;
		
		if stream.client_b.is_some() {
			return Err(Error::StreamAlreadyOpen);
		}
		
		let stream_index = ClientStreamIndex(client.next_stream_index);
		client.next_stream_index += 1;
		
		stream.client_b = Some(StreamClient { client_id, stream_index });
		
		client.streams.insert(stream_index, id);
		
		if let Some(client_a) = self.clients.get_mut(&stream.client_a.client_id) {
			let msg = Message::StreamOpen {
				index: stream.client_a.stream_index,
			};
			let _ = client_a.inbox_tx.unbounded_send(msg);
		}
		
		self.log(LogMessage::StreamOpen { id, index: stream_index, client: client_id });
		
		Ok(stream_index)
	}
	
	fn stream_send(&mut self, index: ClientStreamIndex, data: &[u8], client_id: Uuid) -> Result<(), Error> {
		let client = self.clients.get_mut(&client_id).ok_or(Error::ClientNotFound)?;
		let stream_id = client.streams.get(&index).ok_or(Error::StreamNotFound)?;
		let stream = self.streams.get(&stream_id).ok_or(Error::StreamNotFound)?;
		let recipient = stream.get_other(&client_id, &index).ok_or(Error::StreamNotOpen)?;
		let recipient_client = self.clients.get_mut(&recipient.client_id).ok_or(Error::StreamNotOpen)?;
		
		let stream_index_bytes = recipient.stream_index.0.to_le_bytes();
		let payload = [&stream_index_bytes, data].concat();
		
		let msg = Message::StreamData {
			index: recipient.stream_index,
			data: payload,
		};
		let _ = recipient_client.inbox_tx.unbounded_send(msg);
		
		Ok(())
	}
	
	fn close_stream(&mut self, index: ClientStreamIndex, client_id: Uuid) -> Result<(), Error> {
		let client = self.clients.get_mut(&client_id).ok_or(Error::ClientNotFound)?;
		let stream_id = client.streams.get(&index).ok_or(Error::StreamNotFound)?;
		
		let stream_id2 = stream_id.clone();
		self.close_stream_by_id(stream_id2)
	}
	
	fn close_stream_by_id(&mut self, id: StreamId) -> Result<(), Error> {
		let stream = self.streams.remove(&id).ok_or(Error::StreamNotFound)?;
		
		self.log(LogMessage::StreamClose { id, index: stream.client_a.stream_index, client: stream.client_a.client_id });
		if let Some(client_a) = self.clients.get_mut(&stream.client_a.client_id) {
			client_a.streams.remove(&stream.client_a.stream_index);
			
			let msg = Message::StreamClosed {
				index: stream.client_a.stream_index,
			};
			let _ = client_a.inbox_tx.unbounded_send(msg);
		}
		
		if let Some(stream_client_b) = stream.client_b {
			self.log(LogMessage::StreamClose { id, index: stream_client_b.stream_index, client: stream_client_b.client_id });
			if let Some(client_b) = self.clients.get_mut(&stream_client_b.client_id) {
				client_b.streams.remove(&stream_client_b.stream_index);
				
				let msg = Message::StreamClosed {
					index: stream_client_b.stream_index,
				};
				let _ = client_b.inbox_tx.unbounded_send(msg);
			}
		}
		
		Ok(())
	}
	
	fn log(&mut self, message: LogMessage) {
		self.logger.log(&message);
		
		self.internal_emit("$system", "log", serde_json::to_value(message).unwrap()).unwrap()
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
				streams: HashMap::new(),
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
			invocations: vec![],
			inbox_tx: tx,
			disconnect_commands: vec![],
			next_stream_index: 1,
			streams: HashMap::new(),
		};
		
		state.log(LogMessage::ClientConnect { client: id });
		
		state.clients.insert(id, client);
		
		Client { id, server: self.clone(), inbox_rx: rx }
	}
	
	fn client_disconnect(&self, client_id: Uuid) {
		let mut state = self.shared.state.lock().unwrap();
		
		let client = state.clients.remove(&client_id);
		
		if let Some(client) = client {
			for invocation in client.invocations {
				if let Some(client) = state.clients.get_mut(&invocation.client_id) {
					let msg = Message::InvocationResult {
						request_id: invocation.request_id,
						result: Err(Error::ObjectNotInvocable),
					};
					let _ = client.inbox_tx.unbounded_send(msg);
				}
			}
			
			for (_, stream_id) in client.streams {
				let _ = state.close_stream_by_id(stream_id);
			}
			
			for command in client.disconnect_commands {
				match command {
					Command::Set { name, value } => {
						let _ = state.set(&name, value, client.id);
					},
					Command::Patch { name, value } => {
						let _ = state.patch(&name, value, client.id);
					},
					Command::Remove { name } => {
						let _ = state.remove(&name, client.id);
					},
					Command::Emit { object, event, data } => {
						let _ = state.emit(&object, &event, data, client.id);
					},
				}
			}
		}
		
		state.log(LogMessage::ClientDisconnect { client: client_id });
	}
	
	pub fn set_disconnect_commands(&self, commands: Vec<Command>, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		
		if let Some(client) = state.clients.get_mut(&client.id) {
			client.disconnect_commands = commands;
			Ok(())
		} else {
			Err(Error::ClientNotFound)
		}
	}
	
	pub fn set(&self, name: &str, value: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.set(name, value, client.id)
	}
	
	pub fn patch(&self, name: &str, value: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.patch(name, value, client.id)
	}
	
	pub fn get(&self, pattern: &Pattern, client: &Client) -> Vec<Object> {
		let mut state = self.shared.state.lock().unwrap();
		
		state.log(LogMessage::Get { pattern: pattern.string.clone(), client: client.id });
		
		state.objects.values().filter(|object| {
			pattern.matches(&object.name)
		}).cloned().collect()
	}
	
	pub fn query(&self, pattern: &Pattern, provide_rpc: bool, client: &Client) -> Result<(Uuid, Vec<Object>),Error> {
		let mut state = self.shared.state.lock().unwrap();
		
		let id = Uuid::new_v4();
		
		state.log(LogMessage::Query { pattern: pattern.string.clone(), provide_rpc, query: id, client: client.id });
		
		let objects: Vec<Object> = state.objects.values().filter(|object| {
			pattern.matches(&object.name)
		}).cloned().collect();
		
		if let Some(client) = state.clients.get_mut(&client.id) {
			client.queries.push(Query {
				id,
				pattern: pattern.clone(),
				provide_rpc,
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
		
		let mut invocations: Vec<Invocation> = vec![];
		{
			let client = state.clients.get_mut(&client.id).unwrap();
			
			if let Some(index) = client.queries.iter().position(|query| query.id == query_id) {
				client.queries.remove(index);
				
				// TODO: optimize away the vector and cloning
				client.invocations.retain(|invocation| {
					if invocation.query_id == query_id {
						invocations.push(invocation.clone());
						return false;
					} else {
						return true;
					}
				});
			} else {
				return Err(Error::QueryNotFound)
			}
		}
		
		for invocation in invocations {
			if let Some(client) = state.clients.get_mut(&invocation.client_id) {
				let msg = Message::InvocationResult {
					request_id: invocation.request_id,
					result: Err(Error::ObjectNotInvocable),
				};
				let _ = client.inbox_tx.unbounded_send(msg);
			}
		}
		
		Ok(())
	}
	
	pub fn remove(&self, name: &str, client: &Client) -> Result<bool, Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.remove(name, client.id)
	}
	
	pub fn emit(&self, object: &str, event: &str, data: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.emit(object, event, data, client.id)
	}
	
	pub fn invoke(&self, object: &str, method: &str, args: Value, request_id: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.invoke(object, method, args, request_id, client.id)
	}
	
	pub fn invoke_result(&self, invocation_id: Uuid, result: Value, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		
		state.log(LogMessage::InvokeResult { invocation_id, result: result.clone(), client: client.id });
		
		let invocation: Option<Invocation> = (|| {
			let client = state.clients.get_mut(&client.id).unwrap();
			
			if let Some(index) = client.invocations.iter().position(|invocation| invocation.id == invocation_id) {
				Some(client.invocations.remove(index))
			} else {
				None
			}
		})();
		
		if let Some(invocation) = invocation {
			if let Some(client) = state.clients.get_mut(&invocation.client_id) {
				let msg = Message::InvocationResult {
					request_id: invocation.request_id,
					result: Ok(result),
				};
				let _ = client.inbox_tx.unbounded_send(msg);
				
				Ok(())
			} else {
				// client disconnected -> ignore
				Ok(())
			}
		} else {
			Err(Error::InvocationNotFound)
		}
	}
	
	pub fn create_stream(&self, client: &Client) -> Result<(StreamId, ClientStreamIndex), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.create_stream(client.id)
	}
	
	pub fn open_stream(&self, id: StreamId, client: &Client) -> Result<ClientStreamIndex, Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.open_stream(id, client.id)
	}
	
	pub fn close_stream(&self, index: ClientStreamIndex, client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.close_stream(index, client.id)
	}
	
	pub fn stream_send(&self, index: ClientStreamIndex, data: &[u8], client: &Client) -> Result<(), Error> {
		let mut state = self.shared.state.lock().unwrap();
		state.stream_send(index, data, client.id)
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
		
		let (query_id, objects) = server.query(&Pattern::compile("+/temperature").unwrap(), false, &client2).unwrap();
		
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
		
		let (query_id, _) = server.query(&Pattern::compile("+/temperature").unwrap(), false, &client2).unwrap();
		
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
		
		let existed = server.remove("foo", &client).unwrap();
		assert!(!existed);
	}
	
	#[test]
	fn test_remove_existing() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": 1 }), &client).unwrap();
		
		let existed = server.remove("foo", &client).unwrap();
		assert!(existed);
	}
	
	#[test]
	fn test_remove_query() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("foo", json!({ "bar": 1 }), &client).unwrap();
		
		let mut client = server.client_connect();
		
		let (query_id, _) = server.query(&Pattern::compile("*").unwrap(), false, &client).unwrap();
		
		server.remove("foo", &client).unwrap();
		
		let msg = client.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryRemove { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "foo");
			assert_eq!(object.value, json!({ "bar": 1 }));
		} else {
			assert!(false);
		}
		
		server.set("foo", json!({ "bar": 1 }), &client).unwrap();
		
		let msg = client.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryAdd { query_id: msg_query_id, object } = msg {
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
		
		let (query_id, _) = server.query(&Pattern::compile("*").unwrap(), false, &client).unwrap();
		
		server.emit("gamepad", "buttonpress", json!({ "button": "a" }), &client).unwrap();
		
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
		
		let result = server.emit("gamepad", "buttonpress", json!({ "button": "a" }), &client);
		
		assert_eq!(result, Err(Error::ObjectNotFound));
	}
	
	#[test]
	fn test_invoke_doesnt_exist() {
		let server = create_server();
		let client = server.client_connect();
		
		let result = server.invoke("lamp", "setState", json!({ "on": true }), json!(1), &client);
		
		assert_eq!(result, Err(Error::ObjectNotFound));
	}
	
	#[test]
	fn test_invoke_not_invokable() {
		let server = create_server();
		let client = server.client_connect();
		
		server.set("lamp", json!({ "on": false }), &client).unwrap();
		
		let result = server.invoke("lamp", "setState", json!({ "on": true }), json!(1), &client);
		
		assert_eq!(result, Err(Error::ObjectNotInvocable));
	}
	
	#[test]
	fn test_invoke() {
		let server = create_server();
		let mut provider = server.client_connect();
		let mut consumer = server.client_connect();
		
		server.set("lamp", json!({ "on": false }), &provider).unwrap();
		let (query_id, _) = server.query(&Pattern::compile("lamp").unwrap(), true, &provider).unwrap();
		
		let result = server.invoke("lamp", "setState", json!({ "on": true }), json!(1), &consumer);
		assert_eq!(result, Ok(()));
		
		let msg = provider.inbox_try_next().unwrap().unwrap();
		
		let invocation_id;
		
		if let Message::QueryInvocation { query_id: msg_query_id, invocation_id: msg_invocation_id, object, method, args } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object, "lamp");
			assert_eq!(method, "setState");
			assert_eq!(args, json!({ "on": true }));
			invocation_id = msg_invocation_id;
		} else {
			assert!(false);
			return;
		}
		
		server.invoke_result(invocation_id, json!({ "success": true }), &provider).unwrap();
		
		let msg = consumer.inbox_try_next().unwrap().unwrap();
		
		if let Message::InvocationResult { request_id, result } = msg {
			assert_eq!(request_id, json!(1));
			assert_eq!(result, Ok(json!({ "success": true })));
		} else {
			assert!(false);
		}
	}
	
	#[test]
	fn test_invoke_client_disconnect() {
		let server = create_server();
		let mut provider = server.client_connect();
		let mut consumer = server.client_connect();
		
		server.set("lamp", json!({ "on": false }), &provider).unwrap();
		let (query_id, _) = server.query(&Pattern::compile("lamp").unwrap(), true, &provider).unwrap();
		
		let result = server.invoke("lamp", "setState", json!({ "on": true }), json!(1), &consumer);
		assert_eq!(result, Ok(()));
		
		let msg = provider.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryInvocation { query_id: msg_query_id, invocation_id: _invocation_id, object, method, args } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object, "lamp");
			assert_eq!(method, "setState");
			assert_eq!(args, json!({ "on": true }));
		} else {
			assert!(false);
			return;
		}
		
		// disconnect before providing an invocation result
		drop(provider);
		
		let msg = consumer.inbox_try_next().unwrap().unwrap();
		
		if let Message::InvocationResult { request_id, result } = msg {
			assert_eq!(request_id, json!(1));
			assert_eq!(result, Err(Error::ObjectNotInvocable));
		} else {
			assert!(false);
		}
	}
	
	#[test]
	fn test_invoke_unsubscribe() {
		let server = create_server();
		let mut provider = server.client_connect();
		let mut consumer = server.client_connect();
		
		server.set("lamp", json!({ "on": false }), &provider).unwrap();
		let (query_id, _) = server.query(&Pattern::compile("lamp").unwrap(), true, &provider).unwrap();
		
		let result = server.invoke("lamp", "setState", json!({ "on": true }), json!(1), &consumer);
		assert_eq!(result, Ok(()));
		
		let msg = provider.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryInvocation { query_id: msg_query_id, invocation_id: _invocation_id, object, method, args } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object, "lamp");
			assert_eq!(method, "setState");
			assert_eq!(args, json!({ "on": true }));
		} else {
			assert!(false);
			return;
		}
		
		// unsubscribe before providing an invocation result
		server.unsubscribe(query_id, &provider).unwrap();
		
		let msg = consumer.inbox_try_next().unwrap().unwrap();
		
		if let Message::InvocationResult { request_id, result } = msg {
			assert_eq!(request_id, json!(1));
			assert_eq!(result, Err(Error::ObjectNotInvocable));
		} else {
			assert!(false);
		}
	}
	
	#[test]
	fn test_disconnect_command_set() {
		let server = create_server();
		let mut observer = server.client_connect();
		let device = server.client_connect();
		
		server.set("lamp", json!({ "online": true }), &device).unwrap();
		server.set_disconnect_commands(vec![
			Command::Set {
				name: "lamp".to_string(),
				value: json!({ "online": false }),
			}
		], &device).unwrap();
		
		let (query_id, _) = server.query(&Pattern::compile("lamp").unwrap(), true, &observer).unwrap();
		
		drop(device);
		
		let msg = observer.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryChange { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "lamp");
			assert_eq!(object.value, json!({ "online": false }));
		} else {
			assert!(false);
		}
		
		assert!(observer.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_disconnect_command_patch() {
		let server = create_server();
		let mut observer = server.client_connect();
		let device = server.client_connect();
		
		server.patch("lamp", json!({ "online": true }), &device).unwrap();
		server.set_disconnect_commands(vec![
			Command::Patch {
				name: "lamp".to_string(),
				value: json!({ "online": false }),
			}
		], &device).unwrap();
		
		let (query_id, _) = server.query(&Pattern::compile("lamp").unwrap(), true, &observer).unwrap();
		
		drop(device);
		
		let msg = observer.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryChange { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "lamp");
			assert_eq!(object.value, json!({ "online": false }));
		} else {
			assert!(false);
		}
		
		assert!(observer.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_disconnect_command_remove() {
		let server = create_server();
		let mut observer = server.client_connect();
		let device = server.client_connect();
		
		server.patch("client", json!({ "online": true }), &device).unwrap();
		server.set_disconnect_commands(vec![
			Command::Remove {
				name: "client".to_string(),
			}
		], &device).unwrap();
		
		let (query_id, _) = server.query(&Pattern::compile("client").unwrap(), true, &observer).unwrap();
		
		drop(device);
		
		let msg = observer.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryRemove { query_id: msg_query_id, object } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object.name, "client");
		} else {
			assert!(false);
		}
		
		assert!(observer.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_disconnect_command_emit() {
		let server = create_server();
		let mut observer = server.client_connect();
		let device = server.client_connect();
		
		server.set("lamp", json!({ "on": false }), &device).unwrap();
		server.set_disconnect_commands(vec![
			Command::Emit {
				object: "lamp".to_string(),
				event: "offline".to_string(),
				data: json!({}),
			}
		], &device).unwrap();
		
		let (query_id, _) = server.query(&Pattern::compile("lamp").unwrap(), true, &observer).unwrap();
		
		drop(device);
		
		let msg = observer.inbox_try_next().unwrap().unwrap();
		
		if let Message::QueryEvent { query_id: msg_query_id, object, event, data } = msg {
			assert_eq!(msg_query_id, query_id);
			assert_eq!(object, "lamp");
			assert_eq!(event, "offline");
			assert_eq!(data, json!({}));
		} else {
			assert!(false);
		}
		
		assert!(observer.inbox_try_next().is_err());
	}
	
	#[test]
	fn test_create_stream() {
		let server = create_server();
		let client = server.client_connect();
		
		let (stream1_id, stream1_index) = server.create_stream(&client).unwrap();
		let (stream2_id, stream2_index) = server.create_stream(&client).unwrap();
		assert_eq!(stream1_index.0, 1);
		assert_eq!(stream2_index.0, 2);
		assert!(stream1_id != stream2_id);
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client_state = state.clients.get(&client.id).unwrap();
			assert_eq!(client_state.next_stream_index, 3);
			assert_eq!(client_state.streams.get(&stream1_index).unwrap(), &stream1_id);
			assert_eq!(client_state.streams.get(&stream2_index).unwrap(), &stream2_id);
			
			let stream1 = state.streams.get(&stream1_id).unwrap();
			assert_eq!(stream1.id, stream1_id);
			assert_eq!(stream1.client_a.client_id, client.id);
			assert_eq!(stream1.client_a.stream_index, stream1_index);
			assert!(stream1.client_b.is_none());
			
			let stream2 = state.streams.get(&stream2_id).unwrap();
			assert_eq!(stream2.id, stream2_id);
			assert_eq!(stream2.client_a.client_id, client.id);
			assert_eq!(stream2.client_a.stream_index, stream2_index);
			assert!(stream2.client_b.is_none());
		}
	}
	
	#[test]
	fn test_open_stream() {
		let server = create_server();
		let mut client1 = server.client_connect();
		let client2 = server.client_connect();
		
		let (stream_id, client1_stream_index) = server.create_stream(&client1).unwrap();
		let client2_stream_index = server.open_stream(stream_id, &client2).unwrap();
		
		assert_eq!(client1_stream_index.0, 1);
		assert_eq!(client2_stream_index.0, 1);
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamOpen { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client1_state = state.clients.get(&client1.id).unwrap();
			assert_eq!(client1_state.next_stream_index, 2);
			assert_eq!(client1_state.streams.get(&client1_stream_index).unwrap(), &stream_id);
			
			let client2_state = state.clients.get(&client2.id).unwrap();
			assert_eq!(client2_state.next_stream_index, 2);
			assert_eq!(client2_state.streams.get(&client2_stream_index).unwrap(), &stream_id);
			
			let stream = state.streams.get(&stream_id).unwrap();
			assert_eq!(stream.id, stream_id);
			assert_eq!(stream.client_a.client_id, client1.id);
			assert_eq!(stream.client_a.stream_index, client1_stream_index);
			assert_eq!(stream.client_b.as_ref().unwrap().client_id, client2.id);
			assert_eq!(stream.client_b.as_ref().unwrap().stream_index, client2_stream_index);
		}
	}
	
	#[test]
	fn test_open_stream_self() {
		let server = create_server();
		let mut client = server.client_connect();
		
		let (stream_id, client1_stream_index) = server.create_stream(&client).unwrap();
		let client2_stream_index = server.open_stream(stream_id, &client).unwrap();
		
		assert_eq!(client1_stream_index.0, 1);
		assert_eq!(client2_stream_index.0, 2);
		
		let msg = client.inbox_try_next().unwrap().unwrap();
		if let Message::StreamOpen { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client_state = state.clients.get(&client.id).unwrap();
			assert_eq!(client_state.next_stream_index, 3);
			assert_eq!(client_state.streams.get(&client1_stream_index).unwrap(), &stream_id);
			assert_eq!(client_state.streams.get(&client2_stream_index).unwrap(), &stream_id);
			
			let stream = state.streams.get(&stream_id).unwrap();
			assert_eq!(stream.id, stream_id);
			assert_eq!(stream.client_a.client_id, client.id);
			assert_eq!(stream.client_a.stream_index, client1_stream_index);
			assert_eq!(stream.client_b.as_ref().unwrap().client_id, client.id);
			assert_eq!(stream.client_b.as_ref().unwrap().stream_index, client2_stream_index);
		}
	}
	
	#[test]
	fn test_stream_send_by_client1() {
		let server = create_server();
		let client1 = server.client_connect();
		let mut client2 = server.client_connect();
		
		let (stream_id, client1_stream_index) = server.create_stream(&client1).unwrap();
		let client2_stream_index = server.open_stream(stream_id, &client2).unwrap();
		
		server.stream_send(client1_stream_index, &[1, 2, 3, 4, 5, 6], &client1).unwrap();
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		if let Message::StreamData { index, data } = msg {
			assert_eq!(index.0, 1);
			assert_eq!(data, &[1, 0, 0, 0, 1, 2, 3, 4, 5, 6]);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client1_state = state.clients.get(&client1.id).unwrap();
			assert_eq!(client1_state.next_stream_index, 2);
			assert_eq!(client1_state.streams.get(&client1_stream_index).unwrap(), &stream_id);
			
			let client2_state = state.clients.get(&client2.id).unwrap();
			assert_eq!(client2_state.next_stream_index, 2);
			assert_eq!(client2_state.streams.get(&client2_stream_index).unwrap(), &stream_id);
			
			let stream = state.streams.get(&stream_id).unwrap();
			assert_eq!(stream.id, stream_id);
			assert_eq!(stream.client_a.client_id, client1.id);
			assert_eq!(stream.client_a.stream_index, client1_stream_index);
			assert_eq!(stream.client_b.as_ref().unwrap().client_id, client2.id);
			assert_eq!(stream.client_b.as_ref().unwrap().stream_index, client2_stream_index);
		}
	}
	
	#[test]
	fn test_stream_send_by_client2() {
		let server = create_server();
		let mut client1 = server.client_connect();
		let client2 = server.client_connect();
		
		let (stream_id, client1_stream_index) = server.create_stream(&client1).unwrap();
		let client2_stream_index = server.open_stream(stream_id, &client2).unwrap();
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamOpen { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		server.stream_send(client2_stream_index, &[1, 2, 3, 4, 5, 6], &client2).unwrap();
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamData { index, data } = msg {
			assert_eq!(index.0, 1);
			assert_eq!(data, &[1, 0, 0, 0, 1, 2, 3, 4, 5, 6]);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client1_state = state.clients.get(&client1.id).unwrap();
			assert_eq!(client1_state.next_stream_index, 2);
			assert_eq!(client1_state.streams.get(&client1_stream_index).unwrap(), &stream_id);
			
			let client2_state = state.clients.get(&client2.id).unwrap();
			assert_eq!(client2_state.next_stream_index, 2);
			assert_eq!(client2_state.streams.get(&client2_stream_index).unwrap(), &stream_id);
			
			let stream = state.streams.get(&stream_id).unwrap();
			assert_eq!(stream.id, stream_id);
			assert_eq!(stream.client_a.client_id, client1.id);
			assert_eq!(stream.client_a.stream_index, client1_stream_index);
			assert_eq!(stream.client_b.as_ref().unwrap().client_id, client2.id);
			assert_eq!(stream.client_b.as_ref().unwrap().stream_index, client2_stream_index);
		}
	}
	
	#[test]
	fn test_close_stream_by_client1() {
		let server = create_server();
		let mut client1 = server.client_connect();
		let mut client2 = server.client_connect();
		
		let _ = server.create_stream(&client1).unwrap(); // make sure that stream indices are different
		let (stream_id, client1_stream_index) = server.create_stream(&client1).unwrap();
		let client2_stream_index = server.open_stream(stream_id, &client2).unwrap();
		
		assert_eq!(client1_stream_index.0, 2);
		assert_eq!(client2_stream_index.0, 1);
		
		server.close_stream(client1_stream_index, &client1).unwrap();
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamOpen { index } = msg {
			assert_eq!(index.0, 2);
		} else {
			assert!(false);
		}
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamClosed { index } = msg {
			assert_eq!(index.0, 2);
		} else {
			assert!(false);
		}
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		if let Message::StreamClosed { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client1_state = state.clients.get(&client1.id).unwrap();
			assert_eq!(client1_state.next_stream_index, 3);
			assert!(client1_state.streams.get(&client1_stream_index).is_none());
			
			let client2_state = state.clients.get(&client2.id).unwrap();
			assert_eq!(client2_state.next_stream_index, 2);
			assert!(client2_state.streams.get(&client2_stream_index).is_none());
			
			assert_eq!(state.streams.len(), 1);
		}
	}
	
	#[test]
	fn test_close_stream_by_client2() {
		let server = create_server();
		let mut client1 = server.client_connect();
		let mut client2 = server.client_connect();
		
		let _ = server.create_stream(&client1).unwrap(); // make sure that stream indices are different
		let (stream_id, client1_stream_index) = server.create_stream(&client1).unwrap();
		let client2_stream_index = server.open_stream(stream_id, &client2).unwrap();
		
		assert_eq!(client1_stream_index.0, 2);
		assert_eq!(client2_stream_index.0, 1);
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamOpen { index } = msg {
			assert_eq!(index.0, 2);
		} else {
			assert!(false);
		}
		
		server.close_stream(client2_stream_index, &client2).unwrap();
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamClosed { index } = msg {
			assert_eq!(index.0, 2);
		} else {
			assert!(false);
		}
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		if let Message::StreamClosed { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client1_state = state.clients.get(&client1.id).unwrap();
			assert_eq!(client1_state.next_stream_index, 3);
			assert!(client1_state.streams.get(&client1_stream_index).is_none());
			
			let client2_state = state.clients.get(&client2.id).unwrap();
			assert_eq!(client2_state.next_stream_index, 2);
			assert!(client2_state.streams.get(&client2_stream_index).is_none());
			
			assert_eq!(state.streams.len(), 1);
		}
	}
	
	#[test]
	fn test_close_stream_by_client1_disconnect() {
		let server = create_server();
		let client1 = server.client_connect();
		let mut client2 = server.client_connect();
		
		let (stream_id, _) = server.create_stream(&client1).unwrap();
		let _ = server.open_stream(stream_id, &client2).unwrap();
		
		drop(client1);
		
		let msg = client2.inbox_try_next().unwrap().unwrap();
		if let Message::StreamClosed { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client2_state = state.clients.get(&client2.id).unwrap();
			assert_eq!(client2_state.next_stream_index, 2);
			assert_eq!(client2_state.streams.len(), 0);
			
			assert_eq!(state.streams.len(), 0);
		}
	}
	
	#[test]
	fn test_close_stream_by_client2_disconnect() {
		let server = create_server();
		let mut client1 = server.client_connect();
		let client2 = server.client_connect();
		
		let (stream_id, _) = server.create_stream(&client1).unwrap();
		let _ = server.open_stream(stream_id, &client2).unwrap();
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamOpen { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		drop(client2);
		
		let msg = client1.inbox_try_next().unwrap().unwrap();
		if let Message::StreamClosed { index } = msg {
			assert_eq!(index.0, 1);
		} else {
			assert!(false);
		}
		
		{
			let state = server.shared.state.lock().unwrap();
			
			let client1_state = state.clients.get(&client1.id).unwrap();
			assert_eq!(client1_state.next_stream_index, 2);
			assert_eq!(client1_state.streams.len(), 0);
			
			assert_eq!(state.streams.len(), 0);
		}
	}
}
