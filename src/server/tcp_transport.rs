use crate::json_rpc::RequestMessage;
use crate::server::json_rpc::{handle_message, handle_inbox_message};
use crate::server::Server;
use futures::{StreamExt,SinkExt};
use std::net::SocketAddr;
use tokio::net::{TcpStream, TcpListener};
use tokio_util::codec::{Framed, LinesCodec};

async fn handle_connection(stream: TcpStream, _addr: SocketAddr, server: Server) {
	let mut client = server.client_connect();
	
	let mut lines = Framed::new(stream, LinesCodec::new());
	
	loop {
		tokio::select! {
			Some(msg) = client.inbox_next() => {
				let response = handle_inbox_message(msg);
				let json_string = serde_json::to_string(&response).unwrap();
				lines.send(json_string).await.unwrap();
			},
			result = lines.next() => match result {
				Some(Ok(line)) => {
					if let Ok(request) = serde_json::from_str::<RequestMessage>(&line) {
						let response = handle_message(request, &client, server.clone());
						let json_string = serde_json::to_string(&response).unwrap();
						lines.send(json_string).await.unwrap();
					}
				},
				Some(Err(e)) => {
					println!("error {}", e);
				},
				None => break,
			}
		}
	}
}

pub struct TcpTransport {
	addr: SocketAddr,
	server: Server,
}

impl TcpTransport {
	pub fn new(addr: SocketAddr, server: Server) -> Self {
		TcpTransport { addr, server }
	}
	
	pub async fn serve(&self) {
		println!("tcp transport listening on {}", self.addr);
		
		let listener = TcpListener::bind(self.addr).await.unwrap();
		
		loop {
			let (stream, addr) = listener.accept().await.unwrap();
			
			let server = self.server.clone();
			tokio::spawn(async move {
				handle_connection(stream, addr, server).await;
			});
		}
	}
}
