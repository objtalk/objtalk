use crate::Object;
use crate::server::config::SqliteConfig;
use crate::server::Storage;
use rusqlite::{params, Connection, Result, Error};

pub struct SqliteStorage {
	conn: Connection,
}

impl SqliteStorage {
	pub fn new(conn: Connection) -> Self {
		conn.execute("create table if not exists objects (
			name text primary key,
			value text not null,
			last_modified text not null
		)", []).unwrap();
		
		Self {
			conn
		}
	}
	
	pub fn from_config(config: &SqliteConfig) -> Result<Self, Error> {
		let conn = Connection::open(config.filename.clone()).unwrap();
		
		Ok(SqliteStorage::new(conn))
	}
}

impl Storage for SqliteStorage {
	fn get_objects(&self) -> Vec<Object> {
		let mut stmt = self.conn.prepare("SELECT name, value, last_modified FROM objects").unwrap();
		let iter = stmt.query_map([], |row| {
			let value_str: String = row.get(1).unwrap();
			let value = serde_json::from_str(&value_str).unwrap();
			
			Ok(Object {
				name: row.get(0).unwrap(),
				value,
				last_modified: row.get(2).unwrap(),
			})
		}).unwrap();
		
		iter.collect::<Result<Vec<Object>,rusqlite::Error>>().unwrap()
	}
	
	fn add_object(&self, object: Object) {
		let value = serde_json::to_string(&object.value).unwrap();
		
		self.conn.execute(
			"REPLACE INTO objects (name, value, last_modified) VALUES (?1, ?2, ?3)",
			params![object.name, value, object.last_modified]
		).unwrap();
	}
	
	fn change_object(&self, object: Object) {
		self.add_object(object);
	}
	
	fn remove_object(&self, object: Object) {
		self.conn.execute(
			"DELETE FROM objects WHERE name = ?1",
			params![object.name]
		).unwrap();
	}
}
