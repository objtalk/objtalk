use crate::server::Object;

#[cfg(feature = "sqlite-backend")]
pub mod sqlite;

pub trait Storage {
	fn get_objects(&self) -> Vec<Object>;
	fn add_object(&self, object: Object);
	fn change_object(&self, object: Object);
	fn remove_object(&self, object: Object);
}
