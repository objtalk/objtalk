use crate::json_rpc::*;
use crate::patterns::Pattern;
use crate::server::{Server, Client, Message};
use serde_json::Value;

fn handle_request(request: Request, request_id: Value, client: &Client, server: Server) -> Result<Option<Response>, String> {
	match request {
		Request::Set { name, value } => {
			server.set(&name, value, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Success { success: true }))
		},
		Request::Patch { name, value } => {
			server.patch(&name, value, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Success { success: true }))
		},
		Request::Get { pattern } => {
			let pattern = Pattern::compile(&pattern).map_err(|_| "invalid pattern".to_string())?;
			
			let objects = server.get(&pattern, client);
			Ok(Some(Response::Get { objects }))
		},
		Request::Query { pattern, provide_rpc } => {
			let pattern = Pattern::compile(&pattern).map_err(|_| "invalid pattern".to_string())?;
			
			let (query_id, objects) = server.query(&pattern, provide_rpc, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Query { query_id, objects }))
		},
		Request::Unsubscribe { query_id } => {
			server.unsubscribe(query_id, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Success { success: true }))
		},
		Request::Remove { name } => {
			let existed = server.remove(&name, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Remove { existed }))
		},
		Request::Emit { object, event, data } => {
			server.emit(&object, &event, data, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Success { success: true }))
		},
		Request::Invoke { object, method, args } => {
			server.invoke(&object, &method, args, request_id, client)
				.map_err(|e| e.to_string())?;
			
			Ok(None)
		},
		Request::InvokeResult { invocation_id, result } => {
			server.invoke_result(invocation_id, result, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Success { success: true }))
		},
		Request::SetDisconnectCommands { commands } => {
			server.set_disconnect_commands(commands, client)
				.map_err(|e| e.to_string())?;
			
			Ok(Some(Response::Success { success: true }))
		},
	}
}

pub fn handle_message(req: RequestMessage, client: &Client, server: Server) -> Option<ResponseMessage> {
	match handle_request(req.request, req.id.clone(), client, server) {
		Ok(None) => None,
		Ok(Some(result)) => {
			Some(ResponseMessage {
				request_id: req.id,
				result: Some(result),
				error: None
			})
		},
		Err(e) => {
			Some(ResponseMessage {
				request_id: req.id,
				result: None,
				error: Some(e)
			})
		}
	}
}

pub fn handle_inbox_message(msg: Message) -> EventMessage {
	match msg {
		Message::QueryAdd { query_id, object } => EventMessage::QueryAdd { query_id, object },
		Message::QueryChange { query_id, object } => EventMessage::QueryChange { query_id, object },
		Message::QueryRemove { query_id, object } => EventMessage::QueryRemove { query_id, object },
		Message::QueryEvent { query_id, object, event, data } => EventMessage::QueryEvent { query_id, object, event, data },
		Message::QueryInvocation { query_id, invocation_id, object, method, args } => EventMessage::QueryInvocation { query_id, invocation_id, object, method, args },
		Message::InvocationResult { request_id, result: Ok(result) } => EventMessage::InvocationResult { request_id, result: Some(result), error: None },
		Message::InvocationResult { request_id, result: Err(error) } => EventMessage::InvocationResult { request_id, result: None, error: Some(error.to_string()) },
	}
}
