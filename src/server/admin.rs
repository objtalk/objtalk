use hyper::{Response, Body, header};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Asset {
	compressed: bool,
	data: Vec<u8>,
}

impl Asset {
	fn to_response(&self, mime_type: &str) -> Response<Body> {
		if self.compressed {
			Response::builder()
				.header(header::CONTENT_TYPE, mime_type)
				.header(header::CONTENT_ENCODING, "deflate")
				.body(Body::from(self.data.clone())).unwrap()
		} else {
			Response::builder()
				.header(header::CONTENT_TYPE, mime_type)
				.body(Body::from(self.data.clone())).unwrap()
		}
	}
}

include!(concat!(env!("OUT_DIR"), "/admin_assets.rs"));

fn get_mime_type(path: &Path) -> &'static str {
	match path.extension().and_then(|ext| ext.to_str()) {
		Some("html") => "text/html",
		Some("js") => "application/javascript; charset=UTF-8",
		Some("css") => "text/css; charset=UTF-8",
		Some("woff2") => "font/woff2",
		_ => "application/octet-stream",
	}
}

pub fn get_admin_asset(path: &Path, overrides: &Option<PathBuf>) -> Option<Response<Body>> {
	let filename = path.file_name()?.to_str()?;
	if filename.starts_with(".") {
		return None;
	}
			
	if let Some(overrides_dir) = overrides {
		let mut override_path_builder = overrides_dir.clone();
		override_path_builder.push(path);
		let override_path = override_path_builder.as_path();
		
		if override_path.exists() {
			if let Ok(content) = fs::read(override_path) {
				return Some(Response::builder()
					.header(header::CONTENT_TYPE, get_mime_type(path))
					.body(Body::from(content)).unwrap())
			}
		}
	}
	
	let filename = path.to_str().unwrap();
	let asset = ADMIN_ASSETS.get(filename)?;
	Some(asset.to_response(get_mime_type(path)))
}
