mod db;
mod path_utils;
mod auth;
mod handlers;

use axum::{
    routing::{get, post},
    Router,
};
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
        .route("/login", get(handlers::login_get).post(handlers::login_post))
        .route("/logout", post(handlers::logout_post))
        .route("/files", get(handlers::list_files))
        .route("/files/*path", get(handlers::list_files))
        .route("/download/*path", get(handlers::download_file))
        .route("/upload", post(handlers::upload_file))
        .route("/upload/*path", post(handlers::upload_file))
        .route("/delete/*path", post(handlers::delete_file))
        .route("/admin", get(handlers::admin_panel))
        .route("/admin/users", post(handlers::create_user))
        .route("/admin/users/:id/delete", post(handlers::delete_user))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().unwrap();
    println!("Servidor rodando em http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
