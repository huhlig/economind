use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let dashboard_dir = Path::new(&manifest_dir).parent().unwrap().join("dashboard");

    // Re-run build script when dashboard sources change.
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("src").display());
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("package.json").display());
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("vite.config.ts").display());
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("svelte.config.js").display());

    // Only build the dashboard when not in docs-rs or when SKIP_DASHBOARD is not set.
    if env::var("DOCS_RS").is_ok() || env::var("SKIP_DASHBOARD").is_ok() {
        ensure_stub_build(&dashboard_dir);
        return;
    }

    // Check that node_modules are installed.
    if !dashboard_dir.join("node_modules").exists() {
        let result = Command::new("npm")
            .args(["install", "--prefer-offline"])
            .current_dir(&dashboard_dir)
            .status();
        match result {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("cargo:warning=npm not found — skipping dashboard build");
                ensure_stub_build(&dashboard_dir);
                return;
            }
            Err(e) => panic!("failed to run `npm install`: {e}"),
            Ok(status) => assert!(status.success(), "`npm install` failed"),
        }
    }

    // Build the dashboard.
    let result = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&dashboard_dir)
        .status();
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("cargo:warning=npm not found — skipping dashboard build");
            ensure_stub_build(&dashboard_dir);
        }
        Err(e) => panic!("failed to run `npm run build`: {e}"),
        Ok(status) => assert!(status.success(), "`npm run build` failed"),
    }
}

/// Create a minimal `dashboard/build` directory so that `include_dir!` in
/// `main.rs` always finds a valid directory even when Node.js is unavailable.
/// Has no effect if the directory already exists (e.g. from a prior real build).
fn ensure_stub_build(dashboard_dir: &Path) {
    let build_dir = dashboard_dir.join("build");
    if !build_dir.exists() {
        std::fs::create_dir_all(&build_dir)
            .expect("failed to create stub dashboard/build directory");
        std::fs::write(
            build_dir.join("index.html"),
            "<html><body><p>Dashboard not built. \
             Run <code>npm run build</code> in <code>dashboard/</code>.</p></body></html>",
        )
        .expect("failed to write stub index.html");
    }
}
