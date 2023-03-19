use crate::{StreamId, ClientStreamIndex};
use chrono::Local;
use colored::*;
use serde::Serialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "camelCase")]
pub enum LogMessage {
	ClientConnect { client: Uuid },
	ClientDisconnect { client: Uuid },
	Set { object: String, value: Value, client: Uuid },
	Patch { object: String, value: Value, client: Uuid },
	Get { pattern: String, client: Uuid },
	#[serde(rename_all = "camelCase")]
	Query { pattern: String, provide_rpc: bool, query: Uuid, client: Uuid },
	Unsubscribe { query: Uuid, client: Uuid },
	Remove { object: String, client: Uuid },
	Emit { object: String, event: String, data: Value, client: Uuid },
	#[serde(rename_all = "camelCase")]
	Invoke { object: String, method: String, args: Value, invocation_id: Uuid, client: Uuid },
	#[serde(rename_all = "camelCase")]
	InvokeResult { invocation_id: Uuid, result: Value, client: Uuid },
	#[serde(rename_all = "camelCase")]
	StreamCreate { id: StreamId, index: ClientStreamIndex, client: Uuid },
	#[serde(rename_all = "camelCase")]
	StreamOpen { id: StreamId, index: ClientStreamIndex, client: Uuid },
	#[serde(rename_all = "camelCase")]
	StreamClose { id: StreamId, index: ClientStreamIndex, client: Uuid },
}

pub trait Logger {
	fn log(&self, message: &LogMessage);
}

pub struct NullLogger;

impl Logger for NullLogger {
	fn log(&self, _message: &LogMessage) {}
}

fn short_id(uuid: Uuid) -> String {
	uuid.to_hyphenated().to_string()[..7].to_string()
}

fn short_time() -> String {
	Local::now().format("%H:%M:%S%.6f").to_string()
}

struct UuidColorer {
	colors: Vec<Color>,
	assignments: HashMap<Uuid,Color>,
}

impl UuidColorer {
	fn new() -> Self {
		Self {
			colors: vec![
				Color::Red,
				Color::Green,
				Color::Yellow,
				Color::Blue,
				Color::Magenta,
				Color::Cyan,
				Color::BrightRed,
				Color::BrightGreen,
				Color::BrightYellow,
				Color::BrightBlue,
				Color::BrightMagenta,
				Color::BrightCyan,
			],
			assignments: HashMap::new(),
		}
	}
	
	fn assign_color(&mut self, uuid: Uuid) {
		if !self.colors.is_empty() {
			let color = self.colors.remove(0);
			self.assignments.insert(uuid, color);
		}
	}
	
	fn unassign_color(&mut self, uuid: Uuid) {
		if let Some(color) = self.assignments.remove(&uuid) {
			self.colors.push(color);
		}
	}
	
	fn get_color(&self, uuid: Uuid) -> Color {
		if let Some(color) = self.assignments.get(&uuid) {
			*color
		} else {
			Color::Black
		}
	}
}

pub struct StdoutLogger {
	colorer: RefCell<UuidColorer>,
}

impl StdoutLogger {
	pub fn new() -> Self {
		StdoutLogger {
			colorer: RefCell::new(UuidColorer::new()),
		}
	}
	
	fn print(&self, client: Uuid, text: String) {
		let color = self.colorer.borrow().get_color(client);
		let line = format!("{} {} {}", short_time(), short_id(client).color(color), text);
		
		println!("{}", line);
	}
}

impl Logger for StdoutLogger {
	fn log(&self, message: &LogMessage) {
		match message {
			LogMessage::ClientConnect { client } => {
				self.colorer.borrow_mut().assign_color(*client);
				self.print(*client, format!("connect"));
			},
			LogMessage::ClientDisconnect { client } => {
				self.print(*client, format!("disconnect"));
				self.colorer.borrow_mut().unassign_color(*client);
			},
			LogMessage::Get { pattern, client } => self.print(*client, format!("get {}", pattern)),
			LogMessage::Query { pattern, provide_rpc, query, client } => self.print(*client, format!("query {} -> {} (provide rpc: {})", pattern, short_id(*query), provide_rpc)),
			LogMessage::Unsubscribe { query, client } => self.print(*client, format!("unsubscribe {}", short_id(*query))),
			LogMessage::Set { object, value, client } => self.print(*client, format!("set {} {}", object, value)),
			LogMessage::Patch { object, value, client } => self.print(*client, format!("patch {} {}", object, value)),
			LogMessage::Remove { object, client } => self.print(*client, format!("remove {}", object)),
			LogMessage::Emit { object, event, data, client } => self.print(*client, format!("emit {} {} {}", object, event, data)),
			LogMessage::Invoke { object, method, args, invocation_id, client } => self.print(*client, format!("invoke {} {} {} {}", short_id(*invocation_id), object, method, args)),
			LogMessage::InvokeResult { invocation_id, result, client } => self.print(*client, format!("invoke-result {} {}", short_id(*invocation_id), result)),
			LogMessage::StreamCreate { id, index, client } => self.print(*client, format!("stream-create {} {}", short_id(id.0), index.0)),
			LogMessage::StreamOpen { id, index, client } => self.print(*client, format!("stream-open {} {}", short_id(id.0), index.0)),
			LogMessage::StreamClose { id, index, client } => self.print(*client, format!("stream-close {} {}", short_id(id.0), index.0)),
		}
	}
}
