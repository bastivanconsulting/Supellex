# syntax=docker/dockerfile:1
# supellex — image de l'API Rust (build multi-étage, exécution minimale).
# Contexte de build : racine du dépôt (voir docker-compose.yml).
FROM rust:1.100-bookworm AS build
WORKDIR /app
COPY apps/api/Cargo.toml apps/api/Cargo.lock ./apps/api/
COPY apps/api/.cargo ./apps/api/.cargo
COPY apps/api/migrations ./apps/api/migrations
COPY apps/api/src ./apps/api/src
WORKDIR /app/apps/api
RUN cargo build --release --locked

FROM debian:bookworm-slim
# iputils-ping / net-tools : le module découverte réseau appelle les binaires
# système ping et arp. curl : healthcheck.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates iputils-ping net-tools curl \
 && rm -rf /var/lib/apt/lists/* \
 # exécution sans root ; ping garde cap_net_raw via la file capability du binaire
 && groupadd --system supellex \
 && useradd --system --gid supellex --home /nonexistent --shell /usr/sbin/nologin supellex
COPY --from=build /app/apps/api/target/release/supellex-api /usr/local/bin/supellex-api
ENV API_PORT=8080
EXPOSE 8080
USER supellex
HEALTHCHECK --interval=10s --timeout=5s --retries=6 --start-period=15s \
  CMD curl -fsS http://localhost:8080/health || exit 1
CMD ["supellex-api"]
