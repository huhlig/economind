//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! Economind desktop application — Tauri 2 wrapper around the Axum API server.
//!
//! The Axum server (REST + GraphQL + WebSocket + embedded dashboard) runs as a
//! background task bound to `0.0.0.0:PORT`.  The Tauri webview points at
//! `http://localhost:PORT`, giving the same experience as opening a browser.
//!
//! When the window is closed it is hidden rather than terminated, so the server
//! keeps running and external browser sessions remain active.  The system tray
//! lets the user show the window, open it in a browser, or quit.

use std::net::SocketAddr;

use anyhow::Context;
use economind_config::EconomindConfig;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Managed state — bound server address, available to tray event handlers.
struct ServerAddr(SocketAddr);

fn main() {
    if std::env::args_os().len() > 1 {
        if let Err(e) = economind_cli::run_blocking() {
            eprintln!("{e:?}");
            std::process::exit(1);
        }
        return;
    }

    dotenvy::dotenv().ok();

    if std::env::var("ECONOMIND_API_KEY").is_err() {
        std::env::set_var("ECONOMIND_API_KEY", "economind-local");
        eprintln!("ECONOMIND_API_KEY not set; using local desktop key: economind-local");
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "economind_api=info,economind_tauri=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = EconomindConfig::load().expect("Failed to load Economind config");

    tauri::Builder::default()
        .setup(move |app| {
            // ── Start the Axum server in the background ──────────────────────
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<SocketAddr>();
            let cfg_server = cfg.clone();

            tauri::async_runtime::spawn(async move {
                if let Err(e) = economind_api::serve(cfg_server, Some(ready_tx)).await {
                    tracing::error!("Economind server exited with error: {e}");
                    std::process::exit(1);
                }
            });

            // ── Wait for the server to bind (up to 10 s) ────────────────────
            let bound: SocketAddr = tauri::async_runtime::block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(10), ready_rx)
                    .await
                    .map_err(|_| anyhow::anyhow!("Economind server did not start within 10 seconds"))?
                    .map_err(|_| anyhow::anyhow!("Economind server exited before it was ready"))
            })?;

            tracing::info!("Economind server ready at {bound}");

            // Store the bound address in managed state so tray handlers can read it.
            app.manage(ServerAddr(bound));

            // ── Open the main window pointing at localhost ───────────────────
            let url = format!("http://localhost:{}", bound.port())
                .parse()
                .expect("Failed to parse server URL");

            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("Economind")
                .inner_size(1400.0, 900.0)
                .min_inner_size(900.0, 600.0)
                .resizable(true)
                .build()
                .map_err(|e| anyhow::anyhow!("failed to create Economind webview window: {e}"))?;

            // ── System tray ──────────────────────────────────────────────────
            setup_tray(app).context("failed to create Economind system tray")?;

            Ok(())
        })
        // Hide on close instead of quitting — server keeps running.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("Error running Economind desktop app");
}

fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let browser = MenuItem::with_id(app, "browser", "Open in Browser", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Economind", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &browser, &separator, &quit])?;

    TrayIconBuilder::new()
        .tooltip("Economind")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "browser" => {
                let addr = app.state::<ServerAddr>().0;
                let url = format!("http://localhost:{}", addr.port());
                if let Err(e) = open::that(&url) {
                    tracing::warn!("Could not open browser: {e}");
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Single left-click restores the window.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
