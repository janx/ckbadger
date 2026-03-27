#[path = "src/build_version_format.rs"]
mod build_version_format;

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"));
    let semver = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is not set");
    // branch_name is None on detached HEAD (e.g. tag checkout in CI)
    let branch_name = try_git_stdout(&manifest_dir, &["branch", "--show-current"]);
    let commit_hash = git_stdout(&manifest_dir, &["rev-parse", "--short=12", "HEAD"]);

    assert!(
        !commit_hash.is_empty(),
        "git rev-parse --short HEAD returned an empty commit hash"
    );

    let build_version =
        build_version_format::format_build_version(&semver, branch_name.as_deref(), &commit_hash);

    println!("cargo:rustc-env=CKBADGER_BUILD_VERSION={}", build_version);

    emit_git_rerun_hints(&manifest_dir);
}

fn emit_git_rerun_hints(manifest_dir: &Path) {
    let head_path = resolve_git_path(
        manifest_dir,
        &git_stdout(manifest_dir, &["rev-parse", "--git-path", "HEAD"]),
    );
    println!("cargo:rerun-if-changed={}", head_path.display());

    let packed_refs_path = resolve_git_path(
        manifest_dir,
        &git_stdout(manifest_dir, &["rev-parse", "--git-path", "packed-refs"]),
    );
    println!("cargo:rerun-if-changed={}", packed_refs_path.display());

    if let Some(current_ref) = try_git_stdout(manifest_dir, &["symbolic-ref", "-q", "HEAD"]) {
        let ref_path = resolve_git_path(
            manifest_dir,
            &git_stdout(manifest_dir, &["rev-parse", "--git-path", &current_ref]),
        );
        println!("cargo:rerun-if-changed={}", ref_path.display());
    }
}

fn resolve_git_path(manifest_dir: &Path, git_path: &str) -> PathBuf {
    let path = PathBuf::from(git_path);
    if path.is_absolute() {
        path
    } else {
        manifest_dir.join(path)
    }
}

fn git_stdout(manifest_dir: &Path, args: &[&str]) -> String {
    try_git_stdout(manifest_dir, args).unwrap_or_else(|| {
        panic!(
            "failed to run `git {}` while building ckbadger CLI version metadata",
            args.join(" ")
        )
    })
}

fn try_git_stdout(manifest_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(manifest_dir)
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "failed to execute git while building ckbadger CLI version metadata: {}",
                error
            )
        });

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout)
        .expect("git produced non-UTF-8 output while building ckbadger CLI version metadata");
    let trimmed = stdout.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
