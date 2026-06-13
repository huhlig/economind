use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    // manifest_dir = …/crates/api  →  parent = …/crates  →  parent = …/economind
    let dashboard_dir = Path::new(&manifest_dir)
        .parent().unwrap()  // crates/
        .parent().unwrap()  // economind/
        .join("dashboard");

    // Re-run build script when dashboard sources change.
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("src").display());
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("package.json").display());
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("vite.config.ts").display());
    println!("cargo:rerun-if-changed={}", dashboard_dir.join("svelte.config.js").display());

    // Skip when docs.rs is building or the caller explicitly opts out.
    if env::var("DOCS_RS").is_ok() || env::var("SKIP_DASHBOARD").is_ok() {
        ensure_stub_build(&dashboard_dir);
        return;
    }

    // Install node_modules if missing.
    if !dashboard_dir.join("node_modules").exists() {
        run_npm(&dashboard_dir, &["install", "--prefer-offline"]);
    }

    // Build the SvelteKit dashboard.
    run_npm(&dashboard_dir, &["run", "build"]);
}

/// Run an npm command, panicking on failure.
///
/// On Windows `npm` is a `.cmd` script and must be invoked via `cmd /C`.
fn run_npm(cwd: &Path, args: &[&str]) {
    let status = npm_command()
        .args(args)
        .current_dir(cwd)
        .status()
        .unwrap_or_else(|e| panic!("failed to launch npm {}: {e}", args.join(" ")));

    assert!(
        status.success(),
        "npm {} exited with {}",
        args.join(" "),
        status
    );
}

/// Returns a `Command` that invokes npm, using the correct mechanism per OS.
///
/// - **Windows**: `cmd /C npm` — required because `npm` is a `.cmd` script
///   and `CreateProcess` cannot run `.cmd` files directly.
/// - **Unix**: `npm` directly.
fn npm_command() -> Command {
    if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "npm"]);
        cmd
    } else {
        Command::new("npm")
    }
}

/// Create a minimal `dashboard/build` directory so that `include_dir!` always
/// finds a valid directory even when Node.js is unavailable.  Has no effect if
/// the directory already exists (e.g. from a prior real build).
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
