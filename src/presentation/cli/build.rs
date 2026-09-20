use std::env::var;

fn main() {
	println!("cargo:rerun-if-env-changed=GITHUB_SHA");
	println!(
		"cargo:rustc-env=BUILD_COMMIT={}",
		var("GITHUB_SHA").unwrap_or_else(|_| "unknown".to_owned())
	);
}
