//#![allow(dead_code)]
//#![allow(unused_variables)]
//#![allow(unused_assignments)]
//#![allow(unused_mut)]
//#![allow(unused_imports)]
//#![allow(unused_macros)]
//#![allow(unreachable_code)]
//#![allow(unreachable_patterns)]

use clap::Clap;
use objtalk::client::{HttpClient, Error};

/*
$ objtalk get <pattern>
$ objtalk set <name> <value>
*/

//type Error = Box<dyn std::error::Error + Send + Sync>;

const VERSION_STRING: &'static str = env!("VERSION_STRING");

#[derive(Clap)]
#[clap(version = VERSION_STRING)]
struct Opts {
	#[clap(short, long, default_value = "http://127.0.0.1:3000")]
	url: String,
	#[clap(subcommand)]
	command: Command,
}

#[derive(Clap)]
enum Command {
	Get {
		pattern: String
	},
	Set {
		name: String,
		value: String,
	},
	Patch {
		name: String,
		value: String,
	},
	Remove {
		name: String,
	},
	Emit {
		object: String,
		event: String,
		data: String,
	}
}

async fn do_main() -> Result<(), Error> {
	let opts: Opts = Opts::parse();
	
	let client = HttpClient::new(opts.url);
	
	match opts.command {
		Command::Get { pattern } => {
			let objects = client.get(pattern).await?;
			println!("{}", serde_json::to_string_pretty(&objects).unwrap());
			Ok(())
		},
		Command::Set { name, value } => {
			let value = serde_json::from_str(&value)?;
			client.set(name, value).await?;
			Ok(())
		},
		Command::Patch { name, value } => {
			let value = serde_json::from_str(&value)?;
			client.patch(name, value).await?;
			Ok(())
		},
		Command::Remove { name } => {
			let existed = client.remove(&name).await?;
			if !existed {
				eprintln!("{} doesn't exist", name);
			}
			
			Ok(())
		},
		Command::Emit { object, event, data } => {
			let data = serde_json::from_str(&data)?;
			client.emit(object, event, data).await?;
			Ok(())
		}
	}
}

#[tokio::main]
async fn main() {
	if let Err(error) = do_main().await {
		eprintln!("{}", error);
		std::process::exit(1);
	}
}
