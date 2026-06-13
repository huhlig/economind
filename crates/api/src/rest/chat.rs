//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! POST /api/v1/chat — multi-turn conversation with the Economind agent.
//!
//! Optional fields:
//! - `persona_id` — route the message through a named persona
//!   (e.g. `"quant_analyst"`).  Omit for the default all-tools agent.
//! - `depth` — disclosure depth hint: `"basic"`, `"detailed"`, or `"expert"`.
//!   Ignored when no persona is selected.
//!
//! GET /api/v1/chat/personas — list available personas.

use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use economind_agentic::{ChatMessage, DisclosureContext, RequestedDepth};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ChatRequest {
    /// The user's new message.
    pub message: String,
    /// Prior conversation turns (empty for the first message).
    #[serde(default)]
    pub history: Vec<ChatMessage>,
    /// Optional persona ID (e.g. `"quant_analyst"`).  Uses the default
    /// all-tools agent when omitted.
    pub persona_id: Option<String>,
    /// Disclosure depth hint for the persona.  Ignored without `persona_id`.
    pub depth: Option<RequestedDepth>,
}

#[derive(Serialize)]
pub struct ChatResponse {
    /// The assistant's reply.
    pub message: String,
    /// Full updated history including this turn (ready to send back next call).
    pub history: Vec<ChatMessage>,
    /// Persona that handled this turn, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let Some(svc) = state.chat_service() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Chat agent not configured — set ANTHROPIC_API_KEY to enable"
            })),
        )
            .into_response();
    };

    let result = if let Some(ref pid) = req.persona_id {
        let ctx = DisclosureContext {
            turn_count: req.history.len() / 2,
            requested_depth: req.depth,
        };
        svc.chat_as(pid, &req.message, req.history.clone(), ctx).await
    } else {
        svc.chat(&req.message, req.history.clone()).await
    };

    match result {
        Ok(reply) => {
            let mut history = req.history;
            history.push(ChatMessage { role: "user".into(), content: req.message });
            history.push(ChatMessage { role: "assistant".into(), content: reply.clone() });
            Json(ChatResponse {
                message: reply,
                history,
                persona_id: req.persona_id,
            })
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn list_personas_handler(State(state): State<AppState>) -> impl IntoResponse {
    let Some(svc) = state.chat_service() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Chat agent not configured — set ANTHROPIC_API_KEY to enable"
            })),
        )
            .into_response();
    };

    let personas: Vec<serde_json::Value> = svc
        .list_personas()
        .into_iter()
        .map(|(id, description)| serde_json::json!({ "id": id, "description": description }))
        .collect();

    Json(serde_json::json!({ "personas": personas })).into_response()
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat", post(chat_handler))
        .route("/chat/personas", get(list_personas_handler))
}
