// Build script to extract version from Cargo.toml
// and optionally override patch version from CI/CD pipeline

use std::env;

fn main() {
    // Get version from Cargo.toml
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");

    // Parse version into parts
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        panic!("Invalid version format in Cargo.toml: {}", version);
    }

    let major = parts[0];
    let minor = parts[1];
    let patch = parts[2];

    // Check if CI/CD pipeline wants to override patch version
    // This will be used when GitHub Actions is set up
    let final_patch = env::var("LALA_PATCH_VERSION").unwrap_or_else(|_| patch.to_string());

    let final_version = format!("{}.{}.{}", major, minor, final_patch);

    // Emit as environment variable for compile-time embedding
    println!("cargo:rustc-env=LALA_VERSION={}", final_version);

    // Re-run if Cargo.toml changes
    println!("cargo:rerun-if-changed=Cargo.toml");

    // Re-run if LALA_PATCH_VERSION environment variable changes
    println!("cargo:rerun-if-env-changed=LALA_PATCH_VERSION");
}
