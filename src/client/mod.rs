use crate::Object;
use hyper::body::Buf;
use hyper::Client;
use hyper::{Request, Response, Method, Body, StatusCode};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
	#[error("http error: status code {0}")]
	HttpError(StatusCode),
	//#[error(transparent)]
	#[error("internal http error: {0}")]
	InternalHttpError(#[from] hyper::Error),
	#[error("invalid json: {0}")]
	InternalJsonError(#[from] serde_json::Error),
}

fn status_ok(res: &Response<Body>) -> Result<(), Error> {
	if res.status() != StatusCode::OK {
		Err(Error::HttpError(res.status()).into())
	} else {
		Ok(())
	}
}

#[derive(Serialize)]
struct EmitRequest {
	event: String,
	data: Value,
}

pub struct HttpClient {
	url: String,
}

impl HttpClient {
	pub fn new<S: Into<String>>(url: S) -> Self {
		HttpClient {
			url: url.into(),
		}
	}
	
	pub async fn get<S: Into<String>>(&self, pattern: S) -> Result<Vec<Object>, Error> {
		let client = Client::new();
		
		let url = self.url.to_owned() + "/query?pattern=" + &pattern.into(); // TODO: encodeURIComponent
		let res = client.get(url.parse().unwrap()).await?;
		status_ok(&res)?;
		
		let body = hyper::body::aggregate(res).await?;
		
		let objects = serde_json::from_reader(body.reader())?;
		
		Ok(objects)
	}
	
	pub async fn set<S: Into<String>>(&self, name: S, value: Value) -> Result<(), Error> {
		let client = Client::new();
		
		let value_json = serde_json::to_string(&value)?;
		
		let req = Request::builder()
			.method(Method::POST)
			.uri(self.url.to_owned() + "/objects/" + &name.into())
			.body(Body::from(value_json)).unwrap();
		
		let res = client.request(req).await?;
		status_ok(&res)?;
		
		Ok(())
	}
	
	pub async fn patch<S: Into<String>>(&self, name: S, value: Value) -> Result<(), Error> {
		let client = Client::new();
		
		let value_json = serde_json::to_string(&value)?;
		
		let req = Request::builder()
			.method(Method::PATCH)
			.uri(self.url.to_owned() + "/objects/" + &name.into())
			.body(Body::from(value_json)).unwrap();
		
		let res = client.request(req).await?;
		status_ok(&res)?;
		
		Ok(())
	}
	
	pub async fn remove<S: Into<String>>(&self, name: S) -> Result<bool, Error> {
		let client = Client::new();
		
		let req = Request::builder()
			.method(Method::DELETE)
			.uri(self.url.to_owned() + "/objects/" + &name.into())
			.body(Body::empty()).unwrap();
		
		let res = client.request(req).await?;
		
		match res.status() {
			StatusCode::OK => Ok(true),
			StatusCode::NOT_FOUND => Ok(false),
			_ => Err(Error::HttpError(res.status()))
		}
	}
	
	pub async fn emit<S: Into<String>, S2: Into<String>>(&self, object: S, event: S2, data: Value) -> Result<(), Error> {
		let client = Client::new();
		
		let emit_req = EmitRequest { event: event.into(), data: data.into() };
		let json = serde_json::to_string(&emit_req)?;
		
		let req = Request::builder()
			.method(Method::POST)
			.uri(self.url.to_owned() + "/events/" + &object.into())
			.body(Body::from(json)).unwrap();
		
		let res = client.request(req).await?;
		status_ok(&res)?;
		
		Ok(())
	}
}
