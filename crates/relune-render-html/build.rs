//! Generate `src/js/*.js` from TypeScript before Rust compiles `include_str!` embeddings.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set"));

    for rel in [
        "ts",
        "package.json",
        "pnpm-lock.yaml",
        "esbuild.config.mjs",
        "tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed={rel}");
    }

    assert!(
        pnpm_available(),
        "relune-render-html: `pnpm` not found on PATH. Install Node.js and pnpm, then run \
         `pnpm install --frozen-lockfile && pnpm run build` in `{}`, or use a Rust build \
         environment that provides pnpm.",
        manifest_dir.display(),
    );

    let node_modules = manifest_dir.join("node_modules");
    if !node_modules.is_dir() {
        eprintln!(
            "relune-render-html: installing dependencies (pnpm install --frozen-lockfile)..."
        );
        run_pnpm(
            &manifest_dir,
            &["install", "--frozen-lockfile"],
            "pnpm install --frozen-lockfile",
        );
    }

    eprintln!("relune-render-html: building TypeScript viewer scripts...");
    run_pnpm(&manifest_dir, &["run", "build"], "pnpm run build");

    for rel in [
        "src/js/pan_zoom.js",
        "src/js/search.js",
        "src/js/type_filter.js",
        "src/js/group_toggle.js",
        "src/js/collapse.js",
        "src/js/highlight.js",
        "src/js/minimap.js",
        "src/js/shortcuts.js",
        "src/js/load_motion.js",
    ] {
        let path = manifest_dir.join(rel);
        assert!(
            path.is_file(),
            "relune-render-html: expected `{}` after pnpm run build",
            path.display(),
        );
    }
}

fn pnpm_available() -> bool {
    Command::new("pnpm")
        .args(["--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn run_pnpm(manifest_dir: &Path, args: &[&str], label: &str) {
    let status = Command::new("pnpm")
        .args(args)
        .current_dir(manifest_dir)
        .status()
        .unwrap_or_else(|err| panic!("relune-render-html: failed to spawn {label}: {err}"));

    assert!(
        status.success(),
        "relune-render-html: `{label}` failed with status {status} in {}",
        manifest_dir.display(),
    );
}
