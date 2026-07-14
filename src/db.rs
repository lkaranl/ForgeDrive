use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::env;
use bcrypt::{hash, DEFAULT_COST};

pub async fn init_db() -> SqlitePool {
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
    
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Falha ao conectar no SQLite");

    // Executa migrations embarcadas
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Falha ao rodar migrations");

    // Cria usuário administrador inicial se tabela estiver vazia
    seed_admin_user(&pool).await;

    pool
}

async fn seed_admin_user(pool: &SqlitePool) {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
        .unwrap_or((0,));

    if count.0 == 0 {
        let admin_user = env::var("ADMIN_USER").unwrap_or_else(|_| "admin".to_string());
        let admin_pass = env::var("ADMIN_PASS").unwrap_or_else(|_| "admin123".to_string());
        let hashed_pass = hash(admin_pass.as_bytes(), DEFAULT_COST).expect("Erro ao hashear senha");
        let share_dir = env::var("SHARE_DIR").unwrap_or_else(|_| "/data".to_string());

        sqlx::query(
            "INSERT INTO users (username, password_hash, root_path, allow_read, allow_write, is_admin) VALUES (?, ?, ?, 1, 1, 1)"
        )
        .bind(admin_user)
        .bind(hashed_pass)
        .bind(share_dir)
        .execute(pool)
        .await
        .expect("Falha ao inserir admin inicial");
        println!("Usuário admin inicial cadastrado.");
    }
}
