# Estágio 1: Build estático com MUSL
FROM rust:1.95.0-slim AS builder

RUN apt-get update && apt-get install -y musl-tools musl-dev sqlite3 libsqlite3-dev && rm -rf /var/lib/apt/lists/*

# Configura o target MUSL para o compilador Rust
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /app

# Variável necessária para que o SQLx compile de forma estática offline sem precisar do banco ativo
ENV SQLX_OFFLINE=true

# Copia os arquivos de especificação do projeto
COPY Cargo.toml Cargo.lock ./
COPY templates ./templates
COPY migrations ./migrations

# Cria uma pasta src fake para fazer cache de dependências
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl

# Copia o código real e compila de verdade
COPY src ./src
RUN touch src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl

# Estágio 2: Execução com Alpine Linux (enxuto)
FROM alpine:latest

# Instala bibliotecas necessárias mínimas e fuso horário
RUN apk add --no-cache tzdata

WORKDIR /app

# Copia o binário estático compilado no estágio anterior
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/forgedrive ./forgedrive

# Cria os diretórios para banco de dados e arquivos compartilhados
RUN mkdir -p /app/db /data

EXPOSE 8080

CMD ["./forgedrive"]
