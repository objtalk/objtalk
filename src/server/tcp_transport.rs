use crate::json_rpc::RequestMessage;
use crate::server::json_rpc::{handle_message, handle_inbox_message};
use crate::server::Server;
use futures::{StreamExt,SinkExt};
use std::net::SocketAddr;
use tokio::net::{TcpStream, TcpListener};
use tokio_util::codec::{Framed, LinesCodec};

async fn handle_connection(stream: TcpStream, _addr: SocketAddr, server: Server) -> Result<(), Box<dyn std::error::Error>> {
	let mut client = server.client_connect();
	
	let mut lines = Framed::new(stream, LinesCodec::new());
	
	loop {
		tokio::select! {
			Some(msg) = client.inbox_next() => {
				let response = handle_inbox_message(msg);
				let json_string = serde_json::to_string(&response).unwrap();
				lines.send(json_string).await?;
			},
			result = lines.next() => match result {
				Some(Ok(line)) => {
					match serde_json::from_str::<RequestMessage>(&line) {
						Ok(request) => {
							if let Some(response) = handle_message(request, &client, server.clone()) {
								let json_string = serde_json::to_string(&response).unwrap();
								lines.send(json_string).await?;
							}
						},
						Err(_) => {
							lines.send("{\"type\":\"error\",\"error\":\"invalid message\"}").await?;
						},
					}
				},
				Some(Err(e)) => {
					println!("error {}", e);
				},
				None => break,
			}
		}
	}
	
	Ok(())
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
				if let Err(e) = handle_connection(stream, addr, server).await {
					dbg!(e);
				}
			});
		}
	}
}
