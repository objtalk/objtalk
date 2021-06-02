//#![allow(dead_code)]
//#![allow(unused_variables)]
//#![allow(unused_assignments)]
//#![allow(unused_mut)]
//#![allow(unused_imports)]
//#![allow(unused_macros)]
//#![allow(unreachable_code)]
//#![allow(unreachable_patterns)]

use clap::Clap;
use futures::future::join_all;
use futures::FutureExt;
use objtalk::VERSION_STRING;
use objtalk::server::config::*;
use objtalk::server::http_transport::HttpTransport;
use objtalk::server::logger::StdoutLogger;
use objtalk::server::Server;
use objtalk::server::storage::Storage;
#[cfg(feature = "sqlite-backend")]
use objtalk::server::storage::sqlite::SqliteStorage;
use objtalk::server::tcp_transport::TcpTransport;
use std::fs::read_to_string;
use std::io::{self, Read};

#[derive(Clap)]
#[clap(version = VERSION_STRING)]
struct Opts {
	#[clap(short, long, default_value = "objtalk.toml", about = "filename or - to read from stdin")]
	config: String,
}

async fn do_main() -> Result<(), String> {
	let opts: Opts = Opts::parse();
	
	let config_contents = if opts.config == "-" {
		let mut buffer = String::new();
		io::stdin().read_to_string(&mut buffer).map_err(|e| format!("can't read config from stdin: {}", e))?;
		buffer
	} else {
		read_to_string(opts.config).map_err(|e| format!("can't read config file: {}", e))?
	};
		
	let config: Config = toml::from_str(&config_contents)
		.map_err(|e| format!("invalid config: {}", e))?;
	
	let storage: Option<Box<dyn Storage + Send>> = match config.storage {
		#[cfg(feature = "sqlite-backend")]
		Some(StorageConfig::Sqlite { sqlite: config }) => {
			Some(Box::new(SqliteStorage::from_config(&config).unwrap()))
		},
		#[cfg(not(feature = "sqlite-backend"))]
		Some(StorageConfig::Sqlite { .. }) => {
			panic!("build without sqlite backend support")
		},
		None => None,
	};
	
	let logger = Box::new(StdoutLogger::new());
	
	let server = Server::new(storage, logger);
	
	let mut transports = vec![];
	
	for conf in config.http {
		let transport = HttpTransport::new(conf.addr, server.clone(), conf.allow_origin, conf.admin.enabled, conf.admin.asset_overrides);
		transports.push(async move {
			transport.serve().await;
		}.boxed());
	}
	
	for conf in config.tcp {
		let transport = TcpTransport::new(conf.addr, server.clone());
		transports.push(async move {
			transport.serve().await;
		}.boxed());
	}
	
	join_all(transports).await;
	
	Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
	if let Err(error) = do_main().await {
		eprintln!("{}", error);
		std::process::exit(1);
	}
}
