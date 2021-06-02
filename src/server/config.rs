use std::net::SocketAddr;
use std::path::PathBuf;
use serde::Deserialize;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SqliteConfig {
	pub filename: String,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "backend")]
#[serde(rename_all = "kebab-case")]
pub enum StorageConfig {
	Sqlite { sqlite: SqliteConfig }
}

#[derive(Deserialize, Debug, Default, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct AdminConfig {
	#[serde(default)]
	pub enabled: bool,
	#[serde(default)]
	pub asset_overrides: Option<PathBuf>,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
	pub addr: SocketAddr,
	#[serde(default)]
	pub allow_origin: Option<String>,
	#[serde(default)]
	pub admin: AdminConfig,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TcpConfig {
	pub addr: SocketAddr,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
	pub storage: Option<StorageConfig>,
	#[serde(default)]
	pub http: Vec<HttpConfig>,
	#[serde(default)]
	pub tcp: Vec<TcpConfig>,
}

#[cfg(test)]
mod tests {
	use super::*;
	
	#[test]
	fn test_default() {
		let config: Config = toml::from_str(r#"
		"#).unwrap();
		
		assert_eq!(config.storage, None);
		assert_eq!(config.http, vec![]);
		assert_eq!(config.tcp, vec![]);
	}
	
	#[test]
	fn test_storage_sqlite() {
		let config: Config = toml::from_str(r#"
			[storage]
			backend = "sqlite"
			sqlite.filename = "objtalk.db"
		"#).unwrap();
		
		assert_eq!(config.storage, Some(StorageConfig::Sqlite {
			sqlite: SqliteConfig {
				filename: "objtalk.db".to_string(),
			}
		}));
	}
	
	#[test]
	fn test_http_addr() {
		let config: Config = toml::from_str(r#"
			[[http]]
			addr = "127.0.0.1:4000"
		"#).unwrap();
		
		assert_eq!(config.http, vec![
			HttpConfig {
				addr: "127.0.0.1:4000".parse().unwrap(),
				allow_origin: None,
				admin: AdminConfig {
					enabled: false,
					asset_overrides: None,
				},
			}
		]);
	}
	
	#[test]
	fn test_http_addr_admin() {
		let config: Config = toml::from_str(r#"
			[[http]]
			addr = "127.0.0.1:4000"
			admin.enabled = true
		"#).unwrap();
		
		assert_eq!(config.http, vec![
			HttpConfig {
				addr: "127.0.0.1:4000".parse().unwrap(),
				allow_origin: None,
				admin: AdminConfig {
					enabled: true,
					asset_overrides: None,
				}
			}
		]);
	}
	
	#[test]
	fn test_http_addr_admin_root() {
		let config: Config = toml::from_str(r#"
			[[http]]
			addr = "127.0.0.1:4000"
			admin.enabled = true
			admin.asset-overrides = "assets"
		"#).unwrap();
		
		assert_eq!(config.http, vec![
			HttpConfig {
				addr: "127.0.0.1:4000".parse().unwrap(),
				allow_origin: None,
				admin: AdminConfig {
					enabled: true,
					asset_overrides: Some(PathBuf::from("assets")),
				}
			}
		]);
	}
	
	#[test]
	fn test_http_websocket_allow_origin() {
		let config: Config = toml::from_str(r#"
			[[http]]
			addr = "127.0.0.1:4000"
			allow-origin = "localhost"
		"#).unwrap();
		
		assert_eq!(config.http[0].allow_origin, Some("localhost".to_string()));
	}
	
	#[test]
	fn test_tcp() {
		let config: Config = toml::from_str(r#"
			[[tcp]]
			addr = "127.0.0.1:4000"
		"#).unwrap();
		
		assert_eq!(config.tcp, vec![
			TcpConfig {
				addr: "127.0.0.1:4000".parse().unwrap(),
			}
		]);
	}
	
	#[test]
	fn test_multiple_transports() {
		let config: Config = toml::from_str(r#"
			[[tcp]]
			addr = "127.0.0.1:4000"
			[[tcp]]
			addr = "127.0.0.1:4001"
		"#).unwrap();
		
		assert_eq!(config.tcp, vec![
			TcpConfig {
				addr: "127.0.0.1:4000".parse().unwrap(),
			},
			TcpConfig {
				addr: "127.0.0.1:4001".parse().unwrap(),
			},
		]);
	}
}
