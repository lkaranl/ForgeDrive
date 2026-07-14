use axum::{
    async_trait,
    extract::{FromRequestParts, FromRef},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, Key};
use sqlx::SqlitePool;
use serde::Serialize;

pub const SESSION_COOKIE_NAME: &str = "forgedrive_session";

#[derive(Debug, Clone, Serialize)]
pub struct UserSession {
    pub id: i64,
    pub username: String,
    pub root_path: String,
    pub allow_read: bool,
    pub allow_write: bool,
    pub is_admin: bool,
}

#[async_trait]
impl<S> FromRequestParts<S> for UserSession
where
    SqlitePool: FromRef<S>,
    Key: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let pool = SqlitePool::from_ref(state);
        let jar: PrivateCookieJar = PrivateCookieJar::from_request_parts(parts, state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())?;

        let cookie = jar.get(SESSION_COOKIE_NAME);
        if let Some(cookie) = cookie {
            if let Ok(user_id) = cookie.value().parse::<i64>() {
                // Busca usuário no SQLite
                let user_res: Result<UserSessionDb, _> = sqlx::query_as(
                    "SELECT id, username, root_path, allow_read, allow_write, is_admin FROM users WHERE id = ?"
                )
                .bind(user_id)
                .fetch_one(&pool)
                .await;

                if let Ok(u) = user_res {
                    return Ok(UserSession {
                        id: u.id,
                        username: u.username,
                        root_path: u.root_path,
                        allow_read: u.allow_read != 0,
                        allow_write: u.allow_write != 0,
                        is_admin: u.is_admin != 0,
                    });
                }
            }
        }

        // Se não autenticado, redireciona para a tela de login
        Err(Redirect::to("/login").into_response())
    }
}

// Representação intermediária do SQLite
#[derive(sqlx::FromRow)]
struct UserSessionDb {
    id: i64,
    username: String,
    root_path: String,
    allow_read: i32,
    allow_write: i32,
    is_admin: i32,
}
