use crate::json_rpc::RequestMessage;
use crate::patterns::Pattern;
use crate::server::admin::get_admin_asset;
use crate::server::json_rpc::{handle_message, handle_inbox_message};
use crate::server::{Server, Message};
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Request, Response, Body, StatusCode, Method, HeaderMap, header};
use hyper_tungstenite::{tungstenite, HyperWebsocket, is_upgrade_request};
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use tungstenite::Message as WebsocketMessage;

fn remove_first_slash(string: &str) -> &str {
	let mut chars = string.chars();
	chars.next();
	chars.as_str()
}

fn json_response<T: Serialize>(data: &T) -> Response<Body> {
	let json_str = serde_json::to_string(data).unwrap();
	
	Response::builder()
		.header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
		.body(Body::from(json_str)).unwrap()
}

fn error_response(status: StatusCode, string: String) -> Response<Body> {
	Response::builder()
		.status(status)
		.body(Body::from(string)).unwrap()
}

fn is_event_stream(headers: &HeaderMap) -> bool {
	if let Some(value) = headers.get(header::ACCEPT) {
		if let Ok(str_value) = value.to_str() {
			return str_value == "text/event-stream";
		}
	}
	
	false
}

fn event(name: &str, data: Value) -> String {
	let json_string = serde_json::to_string(&data).unwrap();
	format!("event:{}\ndata:{}\n\n", name, json_string)
}

#[derive(Deserialize)]
struct EmitRequest {
	event: String,
	data: Value,
}

async fn serve_websocket(websocket: HyperWebsocket, server: Server) -> Result<(), Box<dyn std::error::Error>> {
	let mut websocket = websocket.await?;
	
	let mut client = server.client_connect();
	
	loop {
		tokio::select! {
			Some(msg) = client.inbox_next() => {
				let response = handle_inbox_message(msg);
				let json_string = serde_json::to_string(&response).unwrap();
				websocket.send(WebsocketMessage::text(json_string)).await?;
			},
			result = websocket.next() => match result {
				Some(message) => {
					let message = message?;
					
					if let WebsocketMessage::Text(line) = message {
						if let Ok(request) = serde_json::from_str::<RequestMessage>(&line) {
							let response = handle_message(request, &client, server.clone());
							let json_string = serde_json::to_string(&response).unwrap();
							websocket.send(WebsocketMessage::text(json_string)).await?;
						}
					}
				},
				None => break,
			}
		}
	}
	
	Ok(())
}

#[derive(Clone)]
struct RequestHandler {
	server: Server,
	allow_origin: Option<String>,
	admin_enabled: bool,
	admin_asset_overrides: Option<PathBuf>,
}

impl RequestHandler {
	async fn handle_request(&self, req: Request<Body>) -> Response<Body> {
		let path = req.uri().path().to_string();
		let parts: Vec<&str> = path.splitn(3, "/").collect();
		
		match (req.method(), parts[1], parts.get(2)) {
			(&Method::GET, "", None) if is_upgrade_request(&req) => self.handle_websocket(req),
			
			(&Method::GET, "", None) if self.admin_enabled => self.handle_admin_index(req).await,
			(&Method::GET, "_assets", Some(_)) | (&Method::HEAD, "_assets", None) if self.admin_enabled => self.handle_admin_assets(req).await,
			
			(&Method::GET, "objects", Some(name)) => self.handle_get(name),
			(&Method::POST, "objects", Some(name)) => self.handle_set(name, req).await,
			(&Method::PATCH, "objects", Some(name)) => self.handle_patch(name, req).await,
			(&Method::DELETE, "objects", Some(name)) => self.handle_remove(name),
			
			(&Method::POST, "events", Some(name)) => self.handle_emit(name, req).await,
			
			(&Method::GET, "query", None) if is_event_stream(req.headers()) => self.handle_query(req),
			(&Method::GET, "query", None) => self.handle_get_all(req),
			_ => Err((StatusCode::BAD_REQUEST, "bad request".to_string())),
		}.unwrap_or_else(|(status, string)| error_response(status, string))
	}
	
	fn handle_get(&self, name: &str) -> Result<Response<Body>, (StatusCode, String)> {
		let client = self.server.client_connect();
		
		let pattern = Pattern::compile(name)
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid pattern".to_string()))?;
		
		let objects = self.server.get(&pattern, &client);
		
		match objects.as_slice() {
			[object] => Ok(json_response(&object)),
			_ => Err((StatusCode::NOT_FOUND, "not found".to_string())),
		}
	}
	
	fn handle_get_all(&self, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		let client = self.server.client_connect();
		
		let query = req.uri().query().ok_or((StatusCode::BAD_REQUEST, "pattern missing".to_string()))?;
		let pattern_str = query.replace("pattern=", "");
		
		let pattern = Pattern::compile(&pattern_str)
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid pattern".to_string()))?;
		
		let objects = self.server.get(&pattern, &client);
		
		Ok(json_response(&objects))
	}

	async fn handle_set(&self, name: &str, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		let client = self.server.client_connect();
		
		let bytes = hyper::body::to_bytes(req).await
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid body".to_string()))?;
		
		let value = serde_json::from_slice::<Value>(&bytes)
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid json".to_string()))?;
		
		self.server.set(name, value, &client)
			.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
				
		let success: Value = json!({ "success": true });
		Ok(json_response(&success))
	}
	
	async fn handle_patch(&self, name: &str, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		let client = self.server.client_connect();
		
		let bytes = hyper::body::to_bytes(req).await
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid body".to_string()))?;
		
		let value = serde_json::from_slice::<Value>(&bytes)
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid json".to_string()))?;
		
		self.server.patch(name, value, &client)
			.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
				
		let success: Value = json!({ "success": true });
		Ok(json_response(&success))
	}
	
	async fn handle_emit(&self, name: &str, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		let client = self.server.client_connect();
		
		let bytes = hyper::body::to_bytes(req).await
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid body".to_string()))?;
		
		let emit_req = serde_json::from_slice::<EmitRequest>(&bytes)
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid json".to_string()))?;
		
		self.server.emit(name.to_string(), emit_req.event, emit_req.data, &client)
			.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
		
		let success: Value = json!({ "success": true });
		Ok(json_response(&success))
	}

	fn handle_remove(&self, name: &str) -> Result<Response<Body>, (StatusCode, String)> {
		let client = self.server.client_connect();
		
		let existed = self.server.remove(name.to_string(), &client);
		
		if existed {
			let success: Value = json!({ "success": true });
			Ok(json_response(&success))
		} else {
			Err((StatusCode::NOT_FOUND, "not found".to_string()))
		}
	}
	
	fn handle_query(&self, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		let mut client = self.server.client_connect();
		
		let query = req.uri().query().ok_or((StatusCode::BAD_REQUEST, "pattern missing".to_string()))?;
		let pattern_str = query.replace("pattern=", "");
		let pattern = Pattern::compile(&pattern_str)
			.map_err(|_| (StatusCode::BAD_REQUEST, "invalid pattern".to_string()))?;
		
		let (query_id, objects) = self.server.query(&pattern, &client)
			.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
		
		let (mut sender, body) = Body::channel();
		
		tokio::spawn(async move {
			let msg = event("initial", json!({ "objects": objects }));
			if sender.send_data(msg.into()).await.is_err() {
				return;
			}
			
			while let Some(msg) = client.inbox_next().await {
				let out = match msg {
					Message::QueryAdd { query_id: msg_query_id, object } =>
						if query_id == msg_query_id { Some(event("add", json!({ "object": object }))) } else { None },
					Message::QueryChange { query_id: msg_query_id, object } =>
						if query_id == msg_query_id { Some(event("change", json!({ "object": object }))) } else { None },
					Message::QueryRemove { query_id: msg_query_id, object } =>
						if query_id == msg_query_id { Some(event("remove", json!({ "object": object }))) } else { None },
					Message::QueryEvent { query_id: msg_query_id, object, event: event_name, data } =>
						if query_id == msg_query_id { Some(event("event", json!({ "object": object, "event": event_name, "data": data }))) } else { None },
				};
				
				if let Some(msg) = out {
					if sender.send_data(msg.into()).await.is_err() {
						return;
					}
				}
			}
		});
		
		let mut res = Response::builder()
			.header(header::CONTENT_TYPE, "text/event-stream");
		
		if let Some(allow_origin) = &self.allow_origin {
			res = res.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, allow_origin)
		}
		
		Ok(res.body(body).unwrap())
	}
	
	fn handle_websocket(&self, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		let (response, websocket) = hyper_tungstenite::upgrade(req, None).unwrap();
		
		let server = self.server.clone();
		tokio::spawn(async move {
			if let Err(e) = serve_websocket(websocket, server).await {
				dbg!(e);
			}
		});
		
		Ok(response)
	}
	
	async fn handle_admin_assets(&self, req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		get_admin_asset(Path::new(remove_first_slash(req.uri().path())), &self.admin_asset_overrides)
			.ok_or((StatusCode::NOT_FOUND, "not found".to_string()))
	}

	async fn handle_admin_index(&self, _req: Request<Body>) -> Result<Response<Body>, (StatusCode, String)> {
		get_admin_asset(Path::new("index.html"), &self.admin_asset_overrides)
			.ok_or((StatusCode::NOT_FOUND, "not found".to_string()))
	}
}

pub struct HttpTransport {
	addr: SocketAddr,
	request_handler: RequestHandler,
}

impl HttpTransport {
	pub fn new(addr: SocketAddr, server: Server,
		allow_origin: Option<String>,
		admin_enabled: bool, admin_asset_overrides: Option<PathBuf>
	) -> Self {
		HttpTransport {
			addr, 
			request_handler: RequestHandler {
				server,
				allow_origin,
				admin_enabled,
				admin_asset_overrides,
			},
		}
	}
	
	pub async fn serve(&self) {
		println!("http transport listening on http://{}", self.addr);
		
		let request_handler = self.request_handler.clone();
		let make_svc = make_service_fn(move |_conn| {
			let request_handler = request_handler.clone();
			
			async move {
				Ok::<_, Infallible>(service_fn(move |req| {
					let request_handler = request_handler.clone();
					
					async move { Ok::<_, Infallible>(request_handler.handle_request(req).await) }
				}))
			}
		});
		
		let http_server = hyper::Server::bind(&self.addr).serve(make_svc);
		http_server.await.unwrap();
	}
}
