use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use bb8_redis::{bb8, RedisConnectionManager};
use futures_util::stream::StreamExt;
use redis::AsyncCommands;
use serde::Serialize;
use std::env;
use std::net::SocketAddr;
use std::time::Duration;
use tracing::{error, info, instrument};

/// The shared application state, holding the connection pool.
/// Cloning this struct is cheap as it only clones the Arc-wrapped pool.
#[derive(Clone)]
struct AppState {
    pool: bb8::Pool<RedisConnectionManager>,
}

#[tokio::main]
async fn main() {
    // Initialize tracing for structured logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // --- Connection Pool Setup ---
    let valkey_url = env::var("VALKEY_URL").expect("VALKEY_URL must be set");

    // Create a connection manager for the pool
    let manager =
        RedisConnectionManager::new(valkey_url).expect("Failed to create Valkey connection manager");

    // Build the connection pool with configuration
    let pool = bb8::Pool::builder()
        .max_size(20) // Max number of connections in the pool
        .connection_timeout(Duration::from_secs(5)) // Timeout for getting a connection
        .build(manager)
        .await
        .expect("Failed to create Valkey connection pool");

    let app_state = AppState { pool };

    // --- Axum Router Setup ---
    let app = Router::new()
        .route("/", get(handler_get_root))
        .route("/command", post(handler_command))
        .route("/keys/count", get(handler_get_key_count))
        .route("/keys/details", get(handler_get_key_details))
        .route("/keys/clear", delete(handler_clear_all_keys))
        .with_state(app_state);

    // --- Server Startup ---
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port)
        .parse::<SocketAddr>()
        .expect("Invalid port");

    info!("Valkey proxy listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Simple health check endpoint.
async fn handler_get_root() -> &'static str {
    "Valkey Proxy is running and ready."
}

#[derive(serde::Deserialize)]
struct CommandPayload {
    command: String,
    args: Vec<String>,
}

/// Generic handler to execute any Valkey command.
#[instrument(skip(state, payload))]
async fn handler_command(
    State(state): State<AppState>,
    Json(payload): Json<CommandPayload>,
) -> impl IntoResponse {
    let mut con = match state.pool.get().await {
        Ok(con) => con,
        Err(e) => {
            error!("Failed to get Valkey connection from pool: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Could not connect to Valkey" })),
            );
        }
    };

    let mut cmd = redis::cmd(&payload.command);
    for arg in &payload.args {
        cmd.arg(arg);
    }

    match cmd.query_async::<_, redis::Value>(&mut *con).await {
        Ok(result) => (StatusCode::OK, Json(serde_json::to_value(result).unwrap())),
        Err(e) => {
            error!("Command '{}' failed: {}", payload.command, e);
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

/// Handler to get the total number of keys.
#[instrument(skip(state))]
async fn handler_get_key_count(State(state): State<AppState>) -> impl IntoResponse {
    let mut con = match state.pool.get().await {
        Ok(con) => con,
        Err(e) => {
            error!("Failed to get Valkey connection from pool: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Could not connect to Valkey" })),
            );
        }
    };

    match con.dbsize().await {
        Ok(count) => (
            StatusCode::OK,
            Json(serde_json::json!({ "key_count": count })),
        ),
        Err(e) => {
            error!("Failed to execute DBSIZE: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

#[derive(Serialize)]
struct KeyDetails {
    key: String,
    ttl_seconds: i64,
    idle_seconds: i64,
}

/// Handler to get details for all keys using a safe SCAN iterator.
#[instrument(skip(state))]
async fn handler_get_key_details(State(state): State<AppState>) -> Response {
    let mut con = match state.pool.get().await {
        Ok(con) => con,
        Err(e) => {
            error!("Failed to get Valkey connection from pool: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Could not connect to Valkey" })),
            )
                .into_response();
        }
    };

    let mut details = Vec::new();
    // We need a mutable reference to the connection for the iterator.
    let mut con_ref = &mut *con;

    let mut key_iter = match con_ref.scan::<String>().await {
        Ok(iter) => iter,
        Err(e) => {
            error!("Failed to start SCAN: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    while let Some(key) = key_iter.next().await {
        let (ttl, idle): (i64, i64) = match redis::pipe()
            .ttl(&key)
            .object_idletime(&key)
            .query_async(&mut *con) // Use the original connection for pipelined commands
            .await
        {
            Ok(values) => values,
            Err(e) => {
                error!("Failed to get details for key {}: {}", key, e);
                continue; // Skip this key on error
            }
        };

        details.push(KeyDetails {
            key,
            ttl_seconds: ttl,
            idle_seconds: idle,
        });
    }

    Json(details).into_response()
}

/// Handler to clear all keys from the current database.
#[instrument(skip(state))]
async fn handler_clear_all_keys(State(state): State<AppState>) -> impl IntoResponse {
    let mut con = match state.pool.get().await {
        Ok(con) => con,
        Err(e) => {
            error!("Failed to get Valkey connection from pool: {}", e);
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "Could not connect to Valkey" })),
            );
        }
    };

    match redis::cmd("FLUSHDB").query_async::<_, String>(&mut *con).await {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "All keys cleared successfully" })),
        ),
        Err(e) => {
            error!("Failed to execute FLUSHDB: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}
