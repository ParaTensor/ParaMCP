# ParaMCP Multi-stage Dockerfile with cargo-chef dependency caching
# Supports hosting Python and Node.js MCP subprocesses

# ---------- Chef Stage (dependency caching) ----------
FROM rust:1.78-slim AS chef
RUN cargo install cargo-chef
WORKDIR /app

# ---------- Planner Stage ----------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------- Builder Stage ----------
FROM chef AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy the recipe and cook (build) dependencies — this layer is cached if recipe.json doesn't change
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source code and build the application
COPY . .
RUN cargo build --release

# ---------- Runtime Stage ----------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    python3 \
    python3-pip \
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/paramcp /usr/local/bin/paramcp

EXPOSE 8080

ENTRYPOINT ["paramcp"]
CMD ["--transport", "http", "--port", "8080"]
