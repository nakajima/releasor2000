use std::process::Command;

fn git_tag() -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let tag = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tag.is_empty() { None } else { Some(tag) }
}

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/packed-refs");

    let version =
        git_tag().unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=RELEASOR2000_VERSION={version}");
}
