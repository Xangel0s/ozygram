# Etapa de Compilación
FROM rust:1.75-slim AS builder

WORKDIR /usr/src/ozymem

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/ozymem-core/Cargo.toml crates/ozymem-core/Cargo.toml
COPY crates/ozymem-parser/Cargo.toml crates/ozymem-parser/Cargo.toml
COPY crates/ozymem-cli/Cargo.toml crates/ozymem-cli/Cargo.toml
COPY crates/ozymem-server/Cargo.toml crates/ozymem-server/Cargo.toml

# Creación de fuentes temporales para cachear la compilación de dependencias
RUN mkdir -p crates/ozymem-core/src && echo "pub fn dummy() {}" > crates/ozymem-core/src/lib.rs && \
    mkdir -p crates/ozymem-parser/src && echo "pub fn dummy() {}" > crates/ozymem-parser/src/lib.rs && \
    mkdir -p crates/ozymem-cli/src && echo "fn main() {}" > crates/ozymem-cli/src/main.rs && \
    mkdir -p crates/ozymem-server/src && echo "fn main() {}" > crates/ozymem-server/src/main.rs

RUN cargo build --release

# Reemplazo con código fuente real
RUN rm -rf crates/ozymem-core/src crates/ozymem-parser/src crates/ozymem-cli/src crates/ozymem-server/src
COPY crates crates/

RUN cargo build --release --bin ozymem-server

# Etapa de Ejecución
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /usr/src/ozymem/target/release/ozymem-server /app/ozymem-server

ENV PORT=8080
ENV OZYMEM_SERVER_MODE=web
ENV MEMGRAPH_URI=memgraph:7687
ENV MEMGRAPH_USER=admin
ENV MEMGRAPH_PASSWORD=admin
ENV MEMGRAPH_DATABASE=memgraph

EXPOSE 8080

CMD ["/app/ozymem-server", "--web"]
