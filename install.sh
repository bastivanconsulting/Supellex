#!/usr/bin/env bash
# supellex — installateur (idempotent, rejouable sans danger).
#
#   ./install.sh                 # mode docker (défaut) : stack complète compose
#   ./install.sh --mode bare     # build local + services systemd (--systemd) ou nohup
#   ./install.sh --dir /srv/supellex --yes
#
# Prérequis mode docker : docker + plugin compose.
# Prérequis mode bare   : docker (PostgreSQL), cargo (stable), node >= .nvmrc, pnpm 12 (corepack).
#
# L'installateur génère .env à la racine s'il n'existe pas (secrets aléatoires),
# démarre la stack, applique les migrations (au démarrage de l'API) et crée le
# compte admin initial (mot de passe dans .env, à changer au premier login).
set -euo pipefail

MODE=docker
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SYSTEMD=0
ASSUME_YES=0

log()  { printf '\033[1;34m[supellex]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[supellex]\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m[supellex]\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
  exit "${1:-0}"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --mode) MODE="${2:?--mode docker|bare}"; shift 2 ;;
    --dir) DIR="${2:?--dir requis}"; shift 2 ;;
    --systemd) SYSTEMD=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    -h|--help) usage 0 ;;
    *) die "option inconnue : $1 (voir --help)" ;;
  esac
done

cd "$DIR"
[ -f apps/api/Cargo.toml ] || die "lancer depuis la racine du dépôt supellex (apps/api/Cargo.toml introuvable dans $DIR)"

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "commande requise introuvable : $1"; }

gen_secret() { # base64 32 octets, fallback /dev/urandom si pas d'openssl
  if command -v openssl >/dev/null 2>&1; then openssl rand -base64 32; else
    od -vN 32 -An -tx1 /dev/urandom | tr -d ' \n' | { read -r h; printf '%s' "$h"; }
  fi
}

# ---------------------------------------------------------------- .env
if [ ! -f .env ]; then
  log "génération de .env (secrets aléatoires)"
  umask 077
  {
    echo "# supellex — généré par install.sh le $(date -u +%Y-%m-%dT%H:%M:%SZ). Ne pas commiter."
    echo "POSTGRES_PASSWORD=$(gen_secret)"
    echo "DATABASE_URL=postgres://supellex:$(gen_secret)@localhost:5432/supellex"
    echo "# le mot de passe de la base dans DATABASE_URL doit valoir POSTGRES_PASSWORD :"
    echo "# en mode docker, compose reconstruit DATABASE_URL depuis POSTGRES_PASSWORD."
    echo "JWT_SECRET=$(gen_secret)"
    echo "JWT_TTL_SECONDS=3600"
    echo "ADMIN_EMAIL=admin@supellex.local"
    echo "ADMIN_INITIAL_PASSWORD=$(gen_secret)"
    echo "DEVICES_ENCRYPTION_KEY=$(gen_secret)"
    echo "API_PORT=8080"
    echo "WEB_PORT=3000"
    echo "PUBLIC_BASE_URL=http://localhost:3000"
    echo "CORS_ALLOWED_ORIGINS=http://localhost:3000"
    echo "API_INTERNAL_URL=http://localhost:8080"
    echo "LOGIN_MAX_ATTEMPTS=5"
    echo "LOGIN_WINDOW_SECONDS=60"
    echo "RUST_LOG=info,supellex_api=info"
    echo "PING_INTERVAL_SECONDS=300"
    echo "PING_TIMEOUT_MS=1000"
    echo "PING_CONCURRENCY=32"
    echo "SCAN_MAX_HOSTS=4096"
  } > .env
  # DATABASE_URL cohérent avec POSTGRES_PASSWORD (dev hors docker)
  PW="$(grep '^POSTGRES_PASSWORD=' .env | cut -d= -f2-)"
  sed -i.bak "s|^DATABASE_URL=.*|DATABASE_URL=postgres://supellex:${PW}@localhost:5432/supellex|" .env && rm -f .env.bak
  chmod 600 .env
  log ".env créé (chmod 600). Mot de passe admin initial : ADMIN_INITIAL_PASSWORD dans .env"
else
  log ".env existant conservé"
  chmod 600 .env 2>/dev/null || true
fi
# recharger les variables pour la suite
set -a; . ./.env; set +a
: "${POSTGRES_PASSWORD:?POSTGRES_PASSWORD manquant dans .env}"
: "${JWT_SECRET:?JWT_SECRET manquant dans .env}"
: "${ADMIN_INITIAL_PASSWORD:?ADMIN_INITIAL_PASSWORD manquant dans .env}"
WEB_PORT="${WEB_PORT:-3000}"
API_PORT="${API_PORT:-8080}"
PUBLIC_BASE_URL="${PUBLIC_BASE_URL:-http://localhost:$WEB_PORT}"

# ---------------------------------------------------------------- mode docker
if [ "$MODE" = docker ]; then
  need_cmd docker
  docker compose version >/dev/null 2>&1 || die "plugin docker compose requis (docker compose version)"
  log "build + démarrage de la stack (postgres, api, web), premier build un peu long"
  docker compose up -d --build
  log "attente de la santé des services…"
  for i in $(seq 1 60); do
    if curl -fsS "http://localhost:${WEB_PORT}/login" >/dev/null 2>&1; then break; fi
    sleep 2
    [ "$i" = 60 ] && { docker compose ps; die "le front ne répond pas sur http://localhost:${WEB_PORT}/login"; }
  done
  log "supellex tourne :"
  log "  interface : ${PUBLIC_BASE_URL}"
  log "  admin     : ${ADMIN_EMAIL:-admin@supellex.local} / mot de passe = ADMIN_INITIAL_PASSWORD dans .env (à changer au premier login)"
  log "  suite     : connecte-toi, puis configure LDAP / SSO dans « Authentification » (/settings/auth)"
  log "  logs      : docker compose logs -f   |   arrêt : docker compose down"
  exit 0
fi

# ---------------------------------------------------------------- mode bare
[ "$MODE" = bare ] || die "--mode doit valoir docker ou bare"
need_cmd cargo
need_cmd node
need_cmd pnpm
need_cmd docker   # uniquement pour PostgreSQL
log "build de l'API (release)…"
( cd apps/api && cargo build --release --locked )
log "install deps + build du front…"
pnpm install --frozen-lockfile
pnpm --filter supellex-web build
log "démarrage de PostgreSQL (docker compose)…"
docker compose up -d postgres
for i in $(seq 1 30); do
  docker exec supellex-db pg_isready -U supellex -d supellex >/dev/null 2>&1 && break
  sleep 1
done

if [ "$SYSTEMD" = 1 ]; then
  [ "$(id -u)" = 0 ] || die "--systemd requiert root (sudo ./install.sh --mode bare --systemd)"
  log "installation des unités systemd (user système supellex, non-root)"
  id supellex >/dev/null 2>&1 || useradd --system --home "$DIR" --shell /usr/sbin/nologin supellex
  chown -R supellex:supellex "$DIR"
  cat > /etc/systemd/system/supellex-api.service <<EOF
[Unit]
Description=supellex API (Rust)
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
User=supellex
Group=supellex
WorkingDirectory=$DIR/apps/api
EnvironmentFile=$DIR/.env
Environment=PING_INTERVAL_SECONDS=\${PING_INTERVAL_SECONDS:-300}
ExecStart=$DIR/apps/api/target/release/supellex-api
Restart=on-failure
RestartSec=5
# sandbox ; ping a besoin de CAP_NET_RAW (file capability perdue avec NoNewPrivileges)
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true
AmbientCapabilities=CAP_NET_RAW
CapabilityBoundingSet=CAP_NET_RAW

[Install]
WantedBy=multi-user.target
EOF
  cat > /etc/systemd/system/supellex-web.service <<EOF
[Unit]
Description=supellex front (Next.js BFF)
After=network-online.target supellex-api.service
Wants=network-online.target

[Service]
Type=simple
User=supellex
Group=supellex
WorkingDirectory=$DIR
EnvironmentFile=$DIR/.env
Environment=NODE_ENV=production PORT=${WEB_PORT} API_INTERNAL_URL=http://localhost:${API_PORT}
ExecStart=$(command -v pnpm) --filter supellex-web start
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true

[Install]
WantedBy=multi-user.target
EOF
  systemctl daemon-reload
  systemctl enable --now supellex-api.service supellex-web.service
  log "services actifs : supellex-api (127.0.0.1:${API_PORT}) et supellex-web (:${WEB_PORT})"
else
  if [ "$ASSUME_YES" = 1 ] || { read -r -p "démarrer l'API et le front en arrière-plan (nohup) ? [O/n] " ans; [ "${ans:-O}" != n ]; }; then
    ( cd apps/api && PING_INTERVAL_SECONDS="${PING_INTERVAL_SECONDS:-300}" nohup ./target/release/supellex-api > "$DIR/supellex-api.log" 2>&1 & )
    ( cd apps/web && NODE_ENV=production PORT="$WEB_PORT" API_INTERNAL_URL="http://localhost:${API_PORT}" nohup pnpm start > "$DIR/supellex-web.log" 2>&1 & )
    sleep 3
    curl -fsS "http://localhost:${API_PORT}/health" >/dev/null && log "API OK sur 127.0.0.1:${API_PORT} (log: $DIR/supellex-api.log)"
    curl -fsS "http://localhost:${WEB_PORT}/login" >/dev/null && log "front OK sur http://localhost:${WEB_PORT} (log: $DIR/supellex-web.log)"
  fi
fi
log "admin : ${ADMIN_EMAIL:-admin@supellex.local} / ADMIN_INITIAL_PASSWORD dans .env"
