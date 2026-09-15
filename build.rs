fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!(
        "cargo:rustc-env=MODS_BUILD_COMMIT={}",
        std::env::var("GITHUB_SHA").unwrap_or_else(|_| "unknown".to_owned())
    );
}
