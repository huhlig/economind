//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! REST API — all `/api/v1/` routes.

mod backtest;
mod chat;
mod data;
mod instruments;
mod positions;
mod signals;
mod strategy;

use crate::state::AppState;
use axum::Router;

/// Mount all REST route groups.  The resulting router is nested under `/api/v1`
/// in `main.rs`.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(instruments::router())
        .merge(signals::router())
        .merge(positions::router())
        .merge(strategy::router())
        .merge(backtest::router())
        .merge(chat::router())
        .merge(data::router())
}
