# ── Stage 1: build ────────────────────────────────────────────────────────────
FROM rust:1.87-slim AS builder

# System deps for sqlx (OpenSSL / ring) and DuckDB
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency compilation by copying manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/core/Cargo.toml           core/Cargo.toml
COPY crates/db/Cargo.toml             db/Cargo.toml
COPY crates/indicators/Cargo.toml     indicators/Cargo.toml
COPY crates/ingest/Cargo.toml         ingest/Cargo.toml
COPY crates/strategy/Cargo.toml       strategy/Cargo.toml
COPY crates/backtest/Cargo.toml       backtest/Cargo.toml
COPY crates/api/Cargo.toml            api/Cargo.toml
COPY crates/cli/Cargo.toml            cli/Cargo.toml
COPY crates/agentic/Cargo.toml        agentic/Cargo.toml
COPY crates/config/Cargo.toml         config/Cargo.toml
COPY strategies/strategy-momentum/Cargo.toml       strategies/strategy-momentum/Cargo.toml
COPY strategies/strategy-regime/Cargo.toml         strategies/strategy-regime/Cargo.toml
COPY strategies/strategy-mean-reversion/Cargo.toml strategies/strategy-mean-reversion/Cargo.toml
COPY strategies/strategy-trend-follow/Cargo.toml   strategies/strategy-trend-follow/Cargo.toml
COPY strategies/strategy-atr-sizer/Cargo.toml      strategies/strategy-atr-sizer/Cargo.toml
COPY strategies/strategy-kelly-sizer/Cargo.toml    strategies/strategy-kelly-sizer/Cargo.toml

# Stub out all lib/main files so Cargo can resolve the dependency graph
RUN find . -name "Cargo.toml" -not -path "*/target/*" | while read f; do \
      dir=$(dirname "$f"); \
      mkdir -p "$dir/src"; \
      if grep -q '\[\[bin\]\]' "$f"; then \
        echo 'fn main() {}' > "$dir/src/main.rs"; \
      fi; \
      touch "$dir/src/lib.rs"; \
    done

RUN cargo build --release 2>/dev/null || true

# Now copy real source and build for real
COPY . .
# Force rebuild of our crates (touch source files to invalidate stubs)
RUN find . -name "*.rs" -not -path "*/target/*" | xargs touch
RUN cargo build --release --bin economind --bin economind-serve

# ── Stage 2: runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/economind       /usr/local/bin/economind
COPY --from=builder /build/target/release/economind-serve /usr/local/bin/economind-serve

# Default config — operators should mount their own economind.toml or set env vars
COPY economind.toml.example /app/economind.toml.example

EXPOSE 8080 8081

# Default command runs the API server; override for CLI usage
CMD ["/usr/local/bin/economind-serve"]
