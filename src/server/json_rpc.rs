use crate::json_rpc::*;
use crate::patterns::Pattern;
use crate::server::{Server, Client, Message};

fn handle_request(request: Request, client: &Client, server: Server) -> Result<Response, String> {
	match request {
		Request::Set { name, value } => {
			server.set(&name, value, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Response::Success { success: true })
		},
		Request::Patch { name, value } => {
			server.patch(&name, value, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Response::Success { success: true })
		},
		Request::Get { pattern } => {
			let pattern = Pattern::compile(&pattern).map_err(|_| "invalid pattern".to_string())?;
			
			let objects = server.get(&pattern, client);
			Ok(Response::Get { objects })
		},
		Request::Query { pattern } => {
			let pattern = Pattern::compile(&pattern).map_err(|_| "invalid pattern".to_string())?;
			
			let (query_id, objects) = server.query(&pattern, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Response::Query { query_id, objects })
		},
		Request::Unsubscribe { query_id } => {
			server.unsubscribe(query_id, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Response::Success { success: true })
		},
		Request::Remove { name } => {
			let existed = server.remove(name, client);
			Ok(Response::Remove { existed })
		},
		Request::Emit { object, event, data } => {
			server.emit(object, event, data, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Response::Success { success: true })
		},
	}
}

pub fn handle_message(req: RequestMessage, client: &Client, server: Server) -> ResponseMessage {
	match handle_request(req.request, client, server) {
		Ok(result) => {
			ResponseMessage {
				request_id: req.id,
				result: Some(result),
				error: None
			}
		},
		Err(e) => {
			ResponseMessage {
				request_id: req.id,
				result: None,
				error: Some(e)
			}
		}
	}
}

pub fn handle_inbox_message(msg: Message) -> EventMessage {
	match msg {
		Message::QueryAdd { query_id, object } => EventMessage::QueryAdd { query_id, object },
		Message::QueryChange { query_id, object } => EventMessage::QueryChange { query_id, object },
		Message::QueryRemove { query_id, object } => EventMessage::QueryRemove { query_id, object },
		Message::QueryEvent { query_id, object, event, data } => EventMessage::QueryEvent { query_id, object, event, data },
	}
}
