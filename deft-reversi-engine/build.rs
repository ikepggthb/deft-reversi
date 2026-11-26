use std::{path::Path, process::Command};

const SOURCE_PATHS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "deft-reversi-engine",
    "deft-reversi-book",
    "deft-reversi-cli",
    "deft-reversi-train",
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let revision = git_revision().unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=DEFT_GIT_REVISION={revision}");
}

fn git_revision() -> Option<String> {
    let manifest = std::env::current_dir().ok()?;
    let workspace = manifest.parent()?;
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(workspace)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .args(args)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let root = git(&["rev-parse", "--show-toplevel"])?;
    if workspace.canonicalize().ok()? != Path::new(&root).canonicalize().ok()? {
        return None;
    }

    for relative in SOURCE_PATHS {
        let path = workspace.join(relative);
        if path.exists() {
            watch(&path);
        }
    }
    let head_ref = git(&["symbolic-ref", "-q", "HEAD"]);
    for name in ["HEAD", "index", "packed-refs"]
        .into_iter()
        .chain(head_ref.as_deref())
    {
        let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", name]) else {
            continue;
        };
        let path = Path::new(&path);
        if path.exists() {
            watch(path);
        } else if head_ref.as_deref() == Some(name) {
            // A packed branch may get a new loose ref on the next commit.
            if let Some(directory) = path.ancestors().skip(1).find(|p| p.is_dir()) {
                watch(directory);
            }
        }
    }

    let mut revision = git(&["rev-parse", "--short=12", "HEAD"])?;
    let args = ["status", "--porcelain", "--untracked-files=normal", "--"]
        .into_iter()
        .chain(SOURCE_PATHS.iter().copied())
        .collect::<Vec<_>>();
    match git(&args) {
        Some(status) if status.is_empty() => {}
        Some(_) => revision.push_str("-dirty"),
        None => revision.push_str("-status-unknown"),
    }
    Some(revision)
}

fn watch(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}
