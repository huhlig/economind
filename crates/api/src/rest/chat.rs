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
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use economind_agentic::{ChatMessage, DisclosureContext, RequestedDepth};
use tracing::debug;
use economind_db::storage::{ChatMessageRow, ChatSessionRow, ChatStorage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ChatRequest {
    /// The user's new message.
    pub message: String,
    /// Prior conversation turns (empty for the first message).
    #[serde(default)]
    pub history: Vec<ChatMessage>,
    /// Persisted chat session. Omit to create a new session.
    pub session_id: Option<Uuid>,
    /// Optional persona ID (e.g. `"quant_analyst"`).  Uses the default
    /// all-tools agent when omitted.
    pub persona_id: Option<String>,
    /// Disclosure depth hint for the persona.  Ignored without `persona_id`.
    pub depth: Option<RequestedDepth>,
    /// Optional dashboard context injected as a system message at the start
    /// of the conversation (e.g. current page state, open positions, etc.).
    pub context: Option<String>,
}

#[derive(Serialize)]
pub struct ChatResponse {
    /// The assistant's reply.
    pub message: String,
    /// Full updated history including this turn (ready to send back next call).
    pub history: Vec<ChatMessage>,
    /// Persisted session for this conversation.
    pub session: ChatSessionResponse,
    /// Persona that handled this turn, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
}

#[derive(Serialize)]
pub struct ChatSessionResponse {
    pub id: Uuid,
    pub title: String,
    pub persona_id: Option<String>,
    pub depth: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ChatMessageResponse {
    pub id: Uuid,
    pub session_id: Uuid,
    pub ordinal: i32,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct ChatSessionDetailResponse {
    pub session: ChatSessionResponse,
    pub messages: Vec<ChatMessageResponse>,
    pub history: Vec<ChatMessage>,
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let Some(svc) = state.chat_agent().await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "error": "Chat agent not configured — configure an LLM provider in Settings"
            })),
        )
            .into_response();
    };

    let store = state.store();
    let now = chrono::Utc::now();
    let session_id = req.session_id.unwrap_or_else(Uuid::new_v4);
    let stored_messages = match store.list_chat_messages(session_id).await {
        Ok(messages) => messages,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let mut prior_history: Vec<ChatMessage> = if req.session_id.is_some() {
        stored_messages
            .iter()
            .map(|msg| ChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            })
            .collect()
    } else {
        req.history.clone()
    };

    // Inject dashboard context as a system message on every request, replacing any
    // stale injection from a prior turn so the agent always sees current page state.
    prior_history.retain(|m| {
        !(m.role == "system" && m.content.contains("User is currently viewing:"))
    });
    if let Some(ref ctx) = req.context {
        if !ctx.trim().is_empty() {
            prior_history.insert(0, ChatMessage {
                role: "system".into(),
                content: format!(
                    "User is currently viewing:\n\n{ctx}\n\nUse this context to answer their questions accurately."
                ),
            });
        }
    }

    debug!(
        session_id = %session_id,
        history_len = prior_history.len(),
        has_context = req.context.is_some(),
        message = %req.message,
        "chat: sending to LLM"
    );
    if let Some(ref ctx) = req.context {
        debug!(context = %ctx, "chat: injected page context");
    }

    let result = if let Some(ref pid) = req.persona_id {
        let ctx = DisclosureContext {
            turn_count: prior_history.len() / 2,
            requested_depth: req.depth,
        };
        svc.chat_as(pid, &req.message, prior_history.clone(), ctx).await
    } else {
        svc.chat(&req.message, prior_history.clone()).await
    };

    match result {
        Ok(reply) => {
            let mut history = prior_history;
            history.push(ChatMessage {
                role: "user".into(),
                content: req.message,
            });
            history.push(ChatMessage {
                role: "assistant".into(),
                content: reply.clone(),
            });
            let session = ChatSessionRow {
                id: session_id,
                title: session_title(
                    state.store().get_chat_session(session_id).await.ok().flatten(),
                    &history,
                ),
                persona_id: req.persona_id.clone(),
                depth: req.depth.map(|d| match d {
                    RequestedDepth::Basic => "basic".to_string(),
                    RequestedDepth::Detailed => "detailed".to_string(),
                    RequestedDepth::Expert => "expert".to_string(),
                }),
                created_at: now,
                updated_at: chrono::Utc::now(),
            };
            if let Err(e) = store.upsert_chat_session(&session).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response();
            }

            let start_ordinal = stored_messages.len() as i32;
            let message_rows = [
                ChatMessageRow {
                    id: Uuid::new_v4(),
                    session_id,
                    ordinal: start_ordinal,
                    role: "user".to_string(),
                    content: history[history.len() - 2].content.clone(),
                    created_at: now,
                },
                ChatMessageRow {
                    id: Uuid::new_v4(),
                    session_id,
                    ordinal: start_ordinal + 1,
                    role: "assistant".to_string(),
                    content: reply.clone(),
                    created_at: chrono::Utc::now(),
                },
            ];
            if let Err(e) = store.insert_chat_messages(&message_rows).await {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response();
            }

            Json(ChatResponse {
                message: reply,
                history,
                session: chat_session_response(session),
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

async fn list_sessions_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.store().list_chat_sessions(Some(50)).await {
        Ok(sessions) => Json(serde_json::json!({
            "sessions": sessions.into_iter().map(chat_session_response).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn get_session_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let session = match state.store().get_chat_session(id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Chat session not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let messages = match state.store().list_chat_messages(id).await {
        Ok(messages) => messages,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    let history = messages
        .iter()
        .map(|msg| ChatMessage {
            role: msg.role.clone(),
            content: msg.content.clone(),
        })
        .collect();
    Json(ChatSessionDetailResponse {
        session: chat_session_response(session),
        messages: messages.into_iter().map(chat_message_response).collect(),
        history,
    })
    .into_response()
}

async fn list_personas_handler(State(state): State<AppState>) -> impl IntoResponse {
    let personas: Vec<serde_json::Value> = state
        .personas()
        .list_visible()
        .into_iter()
        .map(|(id, name, description)| {
            serde_json::json!({
                "id": id,
                "name": name,
                "description": description,
                "visible": true,
            })
        })
        .collect();

    Json(serde_json::json!({ "personas": personas })).into_response()
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat", post(chat_handler))
        .route("/chat/sessions", get(list_sessions_handler))
        .route("/chat/sessions/{id}", get(get_session_handler))
        .route("/chat/personas", get(list_personas_handler))
}

fn chat_session_response(row: ChatSessionRow) -> ChatSessionResponse {
    ChatSessionResponse {
        id: row.id,
        title: row.title,
        persona_id: row.persona_id,
        depth: row.depth,
        created_at: row.created_at.to_rfc3339(),
        updated_at: row.updated_at.to_rfc3339(),
    }
}

fn chat_message_response(row: ChatMessageRow) -> ChatMessageResponse {
    ChatMessageResponse {
        id: row.id,
        session_id: row.session_id,
        ordinal: row.ordinal,
        role: row.role,
        content: row.content,
        created_at: row.created_at.to_rfc3339(),
    }
}

fn session_title(existing: Option<ChatSessionRow>, history: &[ChatMessage]) -> String {
    if let Some(existing) = existing {
        if existing.title != "New chat" {
            return existing.title;
        }
    }
    history
        .iter()
        .find(|msg| msg.role == "user")
        .map(|msg| {
            let mut title = msg.content.trim().replace(['\r', '\n'], " ");
            if title.len() > 64 {
                title.truncate(64);
            }
            if title.is_empty() {
                "New chat".to_string()
            } else {
                title
            }
        })
        .unwrap_or_else(|| "New chat".to_string())
}
