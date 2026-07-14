mod db;
mod path_utils;
mod auth;

use axum::{routing::get, Router};
use axum_extra::extract::cookie::Key;
use std::net::SocketAddr;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub cookie_key: Key,
}

impl axum::extract::FromRef<AppState> for sqlx::SqlitePool {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

impl axum::extract::FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    
    // Inicializa Tracing de Logs
    tracing_subscriber::fmt::init();

    let pool = db::init_db().await;

    // Se a SESSION_KEY do env não tiver 64 bytes, gera uma aleatória
    let session_key_raw = std::env::var("SESSION_KEY").unwrap_or_default();
    let cookie_key = if session_key_raw.len() >= 64 {
        Key::from(session_key_raw.as_bytes())
    } else {
        Key::generate()
    };

    let state = AppState {
        db: pool,
        cookie_key,
    };
    
    let app = Router::new()
        .route("/health", get(|| async { "OK" }))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
    println!("Servidor rodando em http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
