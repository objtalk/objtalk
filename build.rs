use std::env;
use std::process::Command;
use std::str;
use std::fs;
use std::path::{Path, PathBuf};
use deflate::deflate_bytes;

fn find_files_recursive(dir: PathBuf, files: &mut Vec<PathBuf>) {
	for entry in fs::read_dir(dir).unwrap() {
		let entry = entry.unwrap();
		let file_type = entry.file_type().unwrap();
		let path = entry.path();
		
		let filename = path.as_path().file_name().unwrap().to_str().unwrap();
		
		if !filename.starts_with(".") {
			if file_type.is_file() {
				files.push(path);
			} else if file_type.is_dir() {
				find_files_recursive(path, files);
			}
		}
	}
}

fn get_git_hash() -> String {
	if let Ok(contents) = fs::read_to_string(".cargo_vcs_info.json") {
		if let Some(line) = contents.lines().map(|line| line.trim()).filter(|line| line.starts_with("\"sha1\":")).nth(0) {
			if let Some(hash) = line.split("\"").nth(3) {
				return hash.to_string();
			}
		}
	}
	
	let output = Command::new("git")
		.arg("rev-parse")
		.arg("HEAD")
		.output()
		.unwrap();
	
	str::from_utf8(&output.stdout).unwrap().to_string()
}

fn build_version_string() {
	let git_hash = &get_git_hash()[..7];
	let version = env!("CARGO_PKG_VERSION");
	let target = env::var("TARGET").unwrap();
	let version_string = format!("v{} ({}, {})", version, git_hash, target);
	println!("cargo:rustc-env=VERSION_STRING={}", version_string);
}

#[cfg(feature = "server")]
fn build_admin_assets(admin_dir: PathBuf) {
	let profile = env::var("PROFILE").unwrap();
	let out_dir = env::var_os("OUT_DIR").unwrap();
	
	let dest_path = Path::new(&out_dir).join("admin_assets.rs");
	
	let mut assets_file = String::new();
	assets_file += "lazy_static! {
	static ref ADMIN_ASSETS: HashMap<&'static str, Asset> = {
		let mut assets = HashMap::new();\n\n";
	
	let mut files = vec![];
	find_files_recursive(admin_dir.clone(), &mut files);
	for (index, file) in files.iter().enumerate() {
		let filename = file.as_path().strip_prefix(admin_dir.clone()).unwrap().to_str().unwrap();
		
		if profile == "release" {
			let contents = fs::read(&file).unwrap();
			let compressed = deflate_bytes(&contents);
			let file_dest_path = Path::new(&out_dir).join(format!("{}.deflate", index));
			fs::write(&file_dest_path, compressed).unwrap();
			
			let source_file = fs::canonicalize(file_dest_path).unwrap();
			assets_file += &format!("\t\t\tassets.insert(\"{}\",\n\t\t\t\tAsset {{ compressed: true, data: include_bytes!({:?}).to_vec() }});\n", filename, source_file);
		} else {
			let source_file = fs::canonicalize(file).unwrap();
			assets_file += &format!("\t\t\tassets.insert(\"{}\",\n\t\t\t\tAsset {{ compressed: false, data: include_bytes!({:?}).to_vec() }});\n", filename, source_file);
		}
	}
	
	assets_file += "
		assets	
	};
}\n";
	
	fs::write(&dest_path, assets_file).unwrap();
	
	println!("cargo:rerun-if-changed=admin");
}

fn main() {
	build_version_string();
	
	#[cfg(feature = "server")]
	build_admin_assets("admin".into());
	
	println!("cargo:rerun-if-changed=build.rs");
}
