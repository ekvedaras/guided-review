use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git/refs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=tests");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rustc-env=GUIDED_REVIEW_GIT={}", git_revision());
    println!(
        "cargo:rustc-env=GUIDED_REVIEW_RELEASE_TAG={}",
        release_tag().unwrap_or_default()
    );
}

/// The release this build is: a clean build of the commit its version's tag names.
fn release_tag() -> Option<String> {
    let tag = format!("v{}", std::env::var("CARGO_PKG_VERSION").ok()?);
    let is_clean = git(&["status", "--porcelain"]).is_some_and(|status| status.is_empty());
    let tags_here = git(&["tag", "--points-at", "HEAD"])?;
    (is_clean && tags_here.lines().any(|here| here == tag)).then_some(tag)
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_revision() -> String {
    let Some(sha) = git(&["rev-parse", "--short", "HEAD"]).filter(|sha| !sha.is_empty()) else {
        return "unknown".to_owned();
    };
    let is_dirty = git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty());
    if is_dirty {
        format!("{sha}-dirty")
    } else {
        sha
    }
}
