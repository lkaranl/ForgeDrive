use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, Key};
use sqlx::SqlitePool;
use askama::Template;
use bcrypt::{hash, verify, DEFAULT_COST};
use tokio::fs;
use tokio_util::io::ReaderStream;
use std::sync::Arc;
use std::path::PathBuf;

use crate::auth::UserSession;
use crate::path_utils::validate_and_resolve_path;
use crate::AppState;

pub const SESSION_COOKIE_NAME: &str = "forgedrive_session";

// ESTRUTURAS DE TEMPLATES ASKAMA
#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Breadcrumb {
    pub name: String,
    pub path: String,
}

#[derive(Clone)]
pub struct FileItem {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
    pub size: String,
}

#[derive(Template)]
#[template(path = "files.html")]
pub struct FilesTemplate {
    pub username: String,
    pub is_admin: bool,
    pub allow_write: bool,
    pub current_dir_is_root: bool,
    pub parent_path: String,
    pub breadcrumbs: Vec<Breadcrumb>,
    pub items: Vec<FileItem>,
}

#[derive(Template)]
#[template(path = "admin.html")]
pub struct AdminTemplate {
    pub users: Vec<AdminUserItem>,
}

pub struct AdminUserItem {
    pub id: i64,
    pub username: String,
    pub root_path: String,
    pub allow_read: bool,
    pub allow_write: bool,
    pub is_admin: bool,
}

// HANDLERS

// Login GET
pub async fn login_get(jar: PrivateCookieJar) -> impl IntoResponse {
    if jar.get(SESSION_COOKIE_NAME).is_some() {
        return Redirect::to("/files").into_response();
    }
    Html(LoginTemplate { error: None }.render().unwrap()).into_response()
}

// Login POST
#[derive(serde::Deserialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: Option<String>,
}

pub async fn login_post(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Form(payload): Form<LoginPayload>,
) -> impl IntoResponse {
    let password = payload.password.unwrap_or_default();
    
    let user_res: Result<(i64, String, String), _> = sqlx::query_as(
        "SELECT id, username, password_hash FROM users WHERE username = ?"
    )
    .bind(&payload.username)
    .fetch_one(&state.db)
    .await;

    if let Ok((id, _, hash)) = user_res {
        if verify(password.as_bytes(), &hash).unwrap_or(false) {
            let cookie = Cookie::build((SESSION_COOKIE_NAME, id.to_string()))
                .path("/")
                .http_only(true)
                .build();
            return (jar.add(cookie), Redirect::to("/files")).into_response();
        }
    }

    Html(LoginTemplate { error: Some("Usuário ou senha incorretos".to_string()) }.render().unwrap()).into_response()
}

// Logout POST
pub async fn logout_post(jar: PrivateCookieJar) -> impl IntoResponse {
    let mut cookie = Cookie::build((SESSION_COOKIE_NAME, ""))
        .path("/")
        .http_only(true)
        .build();
    cookie.make_removal();
    (jar.add(cookie), Redirect::to("/login"))
}

// Listagem de Arquivos
pub async fn list_files(
    State(_state): State<AppState>,
    session: UserSession,
    requested_path: Option<Path<String>>,
) -> Result<Html<String>, Response> {
    if !session.allow_read {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado: Permissão de Leitura ausente").into_response());
    }

    let req_path_str = match requested_path {
        Some(Path(p)) => p,
        None => String::new(),
    };

    let absolute_path = validate_and_resolve_path(&session.root_path, &req_path_str)
        .map_err(|e| (StatusCode::FORBIDDEN, e).into_response())?;

    if !absolute_path.exists() {
        if req_path_str.is_empty() {
            let _ = fs::create_dir_all(&absolute_path).await;
        } else {
            return Err((StatusCode::NOT_FOUND, "Caminho não encontrado").into_response());
        }
    }

    let mut items = Vec::new();
    let mut read_dir = fs::read_dir(&absolute_path)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Falha ao ler diretório").into_response())?;

    while let Some(entry) = read_dir.next_entry().await.unwrap_or(None) {
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata = entry.metadata().await.unwrap();
        let is_dir = metadata.is_dir();
        
        let rel_path = if req_path_str.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", req_path_str, name)
        };

        let size = if is_dir {
            "-".to_string()
        } else {
            format_size(metadata.len())
        };

        items.push(FileItem {
            name,
            relative_path: rel_path,
            is_dir,
            size,
        });
    }

    items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));

    let mut crumbs = Vec::new();
    let parts: Vec<&str> = req_path_str.split('/').filter(|s| !s.is_empty()).collect();
    let mut current_acc = String::new();
    for part in parts {
        if !current_acc.is_empty() {
            current_acc.push('/');
        }
        current_acc.push_str(part);
        crumbs.push(Breadcrumb {
            name: part.to_string(),
            path: current_acc.clone(),
        });
    }

    let parent_path = if req_path_str.contains('/') {
        let idx = req_path_str.rfind('/').unwrap();
        req_path_str[..idx].to_string()
    } else {
        String::new()
    };

    let template = FilesTemplate {
        username: session.username,
        is_admin: session.is_admin,
        allow_write: session.allow_write,
        current_dir_is_root: req_path_str.is_empty(),
        parent_path,
        breadcrumbs: crumbs,
        items,
    };

    Ok(Html(template.render().unwrap()))
}

// Download de Arquivo (Stream eficiente)
pub async fn download_file(
    State(_state): State<AppState>,
    session: UserSession,
    Path(requested_path): Path<String>,
) -> Result<Response, Response> {
    if !session.allow_read {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado: Sem permissão de Leitura").into_response());
    }

    let absolute_path = validate_and_resolve_path(&session.root_path, &requested_path)
        .map_err(|e| (StatusCode::FORBIDDEN, e).into_response())?;

    if absolute_path.is_dir() {
        return Err((StatusCode::BAD_REQUEST, "Não é possível baixar um diretório").into_response());
    }

    let file = fs::File::open(&absolute_path)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Arquivo não encontrado").into_response())?;

    let file_name = absolute_path.file_name().unwrap().to_string_lossy().to_string();
    
    let stream = ReaderStream::new(file);
    let body = axum::body::Body::from_stream(stream);

    Ok(Response::builder()
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", file_name),
        )
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .unwrap())
}

// Upload de Arquivos
pub async fn upload_file(
    State(_state): State<AppState>,
    session: UserSession,
    requested_path: Option<Path<String>>,
    mut multipart: Multipart,
) -> Result<StatusCode, Response> {
    if !session.allow_write {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado: Sem permissão de Escrita").into_response());
    }

    let req_path_str = match requested_path {
        Some(Path(p)) => p,
        None => String::new(),
    };

    let base_dir = validate_and_resolve_path(&session.root_path, &req_path_str)
        .map_err(|e| (StatusCode::FORBIDDEN, e).into_response())?;

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if let Some(file_name) = field.file_name() {
            if file_name.is_empty() { continue; }
            let safe_name = file_name.replace(char::is_control, "").replace('/', "_");
            let target_path = base_dir.join(safe_name);

            let mut file = fs::File::create(&target_path)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Falha ao criar arquivo").into_response())?;

            let bytes = field.bytes().await.unwrap();
            tokio::io::copy(&mut &bytes[..], &mut file)
                .await
                .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Falha ao gravar arquivo").into_response())?;
        }
    }

    Ok(StatusCode::OK)
}

// Exclusão de Arquivo/Pasta
pub async fn delete_file(
    State(_state): State<AppState>,
    session: UserSession,
    Path(requested_path): Path<String>,
) -> Result<Redirect, Response> {
    if !session.allow_write {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado: Sem permissão de Escrita").into_response());
    }

    let absolute_path = validate_and_resolve_path(&session.root_path, &requested_path)
        .map_err(|e| (StatusCode::FORBIDDEN, e).into_response())?;

    if !absolute_path.exists() {
        return Err((StatusCode::NOT_FOUND, "Arquivo não encontrado").into_response());
    }

    if absolute_path.is_dir() {
        fs::remove_dir(&absolute_path)
            .await
            .map_err(|_| (StatusCode::BAD_REQUEST, "Falha ao remover diretório. Verifique se ele está vazio.").into_response())?;
    } else {
        fs::remove_file(&absolute_path)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Falha ao remover arquivo").into_response())?;
    }

    let mut parent = String::new();
    if requested_path.contains('/') {
        let idx = requested_path.rfind('/').unwrap();
        parent = format!("/{}", &requested_path[..idx]);
    }

    Ok(Redirect::to(&format!("/files{}", parent)))
}

// Admin Panel (Listagem)
pub async fn admin_panel(
    State(state): State<AppState>,
    session: UserSession,
) -> Result<Html<String>, Response> {
    if !session.is_admin {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado: Área restrita ao Admin").into_response());
    }

    let users_db: Vec<AdminUserDb> = sqlx::query_as(
        "SELECT id, username, root_path, allow_read, allow_write, is_admin FROM users"
    )
    .fetch_all(&state.db)
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Erro ao listar usuários").into_response())?;

    let users = users_db.into_iter().map(|u| AdminUserItem {
        id: u.id,
        username: u.username,
        root_path: u.root_path,
        allow_read: u.allow_read != 0,
        allow_write: u.allow_write != 0,
        is_admin: u.is_admin != 0,
    }).collect();

    Ok(Html(AdminTemplate { users }.render().unwrap()))
}

#[derive(sqlx::FromRow)]
pub struct AdminUserDb {
    pub id: i64,
    pub username: String,
    pub root_path: String,
    pub allow_read: i32,
    pub allow_write: i32,
    pub is_admin: i32,
}

#[derive(serde::Deserialize)]
pub struct CreateUserPayload {
    pub username: String,
    pub password: Option<String>,
    pub root_path: String,
    pub allow_read: Option<String>,
    pub allow_write: Option<String>,
}

pub async fn create_user(
    State(state): State<AppState>,
    session: UserSession,
    Form(payload): Form<CreateUserPayload>,
) -> Result<Redirect, Response> {
    if !session.is_admin {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado").into_response());
    }

    let password = payload.password.unwrap_or_default();
    let hashed_pass = hash(password.as_bytes(), DEFAULT_COST).unwrap();
    let read_val = if payload.allow_read.is_some() { 1 } else { 0 };
    let write_val = if payload.allow_write.is_some() { 1 } else { 0 };

    sqlx::query(
        "INSERT INTO users (username, password_hash, root_path, allow_read, allow_write, is_admin) VALUES (?, ?, ?, ?, ?, 0)"
    )
    .bind(payload.username)
    .bind(hashed_pass)
    .bind(payload.root_path)
    .bind(read_val)
    .bind(write_val)
    .execute(&state.db)
    .await
    .map_err(|_| (StatusCode::BAD_REQUEST, "Usuário já existe ou dados inválidos").into_response())?;

    Ok(Redirect::to("/admin"))
}

pub async fn delete_user(
    State(state): State<AppState>,
    session: UserSession,
    Path(id): Path<i64>,
) -> Result<Redirect, Response> {
    if !session.is_admin {
        return Err((StatusCode::FORBIDDEN, "Acesso Negado").into_response());
    }

    if id == session.id {
        return Err((StatusCode::BAD_REQUEST, "Não é possível remover o próprio usuário ativo").into_response());
    }

    sqlx::query("DELETE FROM users WHERE id = ? AND is_admin = 0")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Falha ao remover usuário").into_response())?;

    Ok(Redirect::to("/admin"))
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
