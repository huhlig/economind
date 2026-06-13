//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//

//! GraphQL layer — async-graphql schema, mounted at `/api/v1/graphql`.
//! GraphiQL playground available at `/api/v1/graphiql` (all environments for now).
//!
//! Schema root:
//! - `Query`    — read-only access to all platform data (§5.C.2)
//! - `Mutation` — state-changing operations (§5.C.3)

mod mutation;
mod query;
mod types;

use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};

use crate::state::AppState;

pub use self::mutation::MutationRoot;
pub use self::query::QueryRoot;

/// The concrete schema type used throughout the API layer.
pub type ApiSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the GraphQL schema, injecting shared `AppState`.
pub fn build_schema(state: AppState) -> ApiSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription)
        .data(state)
        .finish()
}

/// Mount `/graphql` (POST) and `/graphiql` (GET) routes under the API prefix.
/// The schema already carries `AppState` in its data layer (injected in `build_schema`).
pub fn router(schema: ApiSchema) -> Router<AppState> {
    // We need the schema accessible inside the handler.  Store it via an
    // `Extension` layer so it coexists with the `AppState` router state.
    use axum::Extension;
    Router::new()
        .route("/graphql", post(graphql_handler))
        .route("/graphiql", get(graphiql_handler))
        .layer(Extension(schema))
}

async fn graphql_handler(
    axum::Extension(schema): axum::Extension<ApiSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    schema.execute(req.into_inner()).await.into()
}

async fn graphiql_handler() -> impl IntoResponse {
    Html(async_graphql::http::graphiql_source("/api/v1/graphql", None))
}
