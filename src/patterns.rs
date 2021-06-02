use regex::{escape, Regex};

#[derive(Debug, Clone)]
pub struct Pattern {
	regex: Regex,
	pub string: String,
	multiple: bool,
	includes_system: bool,
}

impl Pattern {
	pub fn compile(string: &str) -> Result<Pattern,String> {
		let mut multiple = false;
		let mut includes_system = false;
		
		let pattern_string = string.split(",").map(|sub_pattern| {
			"(^".to_owned() + &sub_pattern.split("/").map(|part| {
				match part {
					"*" => {
						multiple = true;
						".+".to_string()
					},
					"+" => {
						multiple = true;
						"[^/]+".to_string()
					},
					"$system" => {
						includes_system = true;
						escape(string)
					},
					string => escape(string),
				}
			}).collect::<Vec<String>>().join("/") + "$)"
		}).collect::<Vec<String>>().join("|");
		
		if let Ok(regex) = Regex::new(&pattern_string) {
			Ok(Pattern { regex, string: string.to_string(), multiple, includes_system })
		} else {
			Err("invalid pattern".to_string())
		}
	}
	
	pub fn matches(&self, string: &String) -> bool {
		if string == "$system" {
			self.includes_system
		} else {
			self.regex.is_match(&string)
		}
	}
	
	pub fn matches_str(&self, string: &str) -> bool {
		if string == "$system" {
			self.includes_system
		} else {
			self.regex.is_match(string)
		}
	}
	
	pub fn matches_multiple(&self) -> bool {
		self.multiple
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	
	#[test]
	fn test_patterns() {
		assert_eq!(Pattern::compile("*").unwrap().regex.as_str(), "(^.+$)");
		assert_eq!(Pattern::compile("+").unwrap().regex.as_str(), "(^[^/]+$)");
		assert_eq!(Pattern::compile("livingroom").unwrap().regex.as_str(), "(^livingroom$)");
		assert_eq!(Pattern::compile("livingroom/+").unwrap().regex.as_str(), "(^livingroom/[^/]+$)");
		assert_eq!(Pattern::compile("livingroom/*").unwrap().regex.as_str(), "(^livingroom/.+$)");
		assert_eq!(Pattern::compile("+/temperature,+/humidity").unwrap().regex.as_str(), "(^[^/]+/temperature$)|(^[^/]+/humidity$)");
		assert_eq!(Pattern::compile(".*").unwrap().regex.as_str(), "(^\\.\\*$)");
		
		assert!(Pattern::compile("livingroom").unwrap().regex.is_match("livingroom"));
		assert!(!Pattern::compile("livingroom").unwrap().regex.is_match("foo/livingroom"));
		assert!(!Pattern::compile(".*").unwrap().regex.is_match("foo"));
		
		assert!(Pattern::compile("device/lamp/+,room/*").unwrap().matches_str("device/lamp/foo"));
		assert!(Pattern::compile("device/lamp/+,room/*").unwrap().matches_str("room/bar"));
		assert!(!Pattern::compile("device/lamp/+,room/*").unwrap().matches_str("scene/livingroom/test"));
	}
}
