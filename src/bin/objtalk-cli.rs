use clap::Clap;
use objtalk::client::{HttpClient, Error};

/*
$ objtalk get <pattern>
$ objtalk set <name> <value>
*/

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
	},
	Invoke {
		object: String,
		method: String,
		args: String,
	},
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
		},
		Command::Invoke { object, method, args } => {
			let args = serde_json::from_str(&args)?;
			let result = client.invoke(object, method, args).await?;
			println!("{}", serde_json::to_string_pretty(&result).unwrap());
			Ok(())
		},
	}
}

#[tokio::main]
async fn main() {
	if let Err(error) = do_main().await {
		eprintln!("{}", error);
		std::process::exit(1);
	}
}
