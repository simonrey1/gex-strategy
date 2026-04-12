#!/usr/bin/env bash
set -euo pipefail

# ── Shared config (single source of truth) ───────────────────────────────────
# https://github.com/gnzsnz/ib-gateway-docker
# socat remaps:  internal 127.0.0.1:4002 (paper) → 0.0.0.0:4004
#                internal 127.0.0.1:4001 (live)  → 0.0.0.0:4003
#
# Theta Terminal: default download is ThetaData *unstable* JAR → .thetadata/ (gitignored).
# Stable channel is an older, smaller build (~12 MB, May 2025); unstable is current (~40 MB, Jul 2025)
# and matches Theta's v3 REST docs. Override: THETA_JAR_URL=https://download-stable.thetadata.us/ThetaTerminalv3.jar
# Same pod network → strategy talks to Theta on 127.0.0.1:25503 (current unstable JAR default HTTP; no --config).
#
# Cheap VPS: Hetzner Cloud CX22+ (Ubuntu 24.04), sudo apt install -y podman,
# clone repo, export TWS_* , kube/play.sh prod (needs curl or wget). Prefer VPN;
# do not expose 5900/25503 publicly.
export GATEWAY_IMAGE="ghcr.io/gnzsnz/ib-gateway:stable"
export THETA_IMAGE="docker.io/eclipse-temurin:21-jre"
export STRATEGY_IMAGE="localhost/gex-strategy:latest"
THETA_JAR_URL="${THETA_JAR_URL:-https://download-unstable.thetadata.us/ThetaTerminalv3.jar}"
export PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
THETA_JAR_DIR="${PROJECT_DIR}/.thetadata"
THETA_JAR_PATH="${THETA_JAR_DIR}/ThetaTerminalv3.jar"
KUBE_DIR="$(cd "$(dirname "$0")" && pwd)"

ENVSUBST_VARS='${GATEWAY_IMAGE} ${THETA_IMAGE} ${STRATEGY_IMAGE} ${PROJECT_DIR} ${TWS_USERID} ${TWS_PASSWORD} ${STRATEGY_CMD} ${DASH_PORT} ${DASH_SCHEME}'

render() {
  local tpl="${KUBE_DIR}/$1"
  while IFS= read -r line; do
    if [[ "$line" =~ ^#\ @include\ (.+)$ ]]; then
      cat "${KUBE_DIR}/inc/${BASH_REMATCH[1]}"
    else
      printf '%s\n' "$line"
    fi
  done < "$tpl" | envsubst "${ENVSUBST_VARS}"
}

# ── Prereqs ──────────────────────────────────────────────────────────────────

require_creds() {
  : "${TWS_USERID:?Set TWS_USERID env var (IBKR username)}"
  : "${TWS_PASSWORD:?Set TWS_PASSWORD env var (IBKR password)}"
}

ensure_theta_jar() {
  mkdir -p "${THETA_JAR_DIR}"
  echo "Fetching Theta Terminal… (${THETA_JAR_URL})"
  local tmp="${THETA_JAR_PATH}.part"
  if command -v curl >/dev/null 2>&1; then
    if curl -fsSL -o "$tmp" "${THETA_JAR_URL}"; then
      mv -f "$tmp" "${THETA_JAR_PATH}"
      echo "Theta Terminal: ${THETA_JAR_PATH}"
      return 0
    fi
  else
    echo "Need curl to download Theta Terminal." >&2
  fi
  rm -f "$tmp"
  if [[ -f "${THETA_JAR_PATH}" ]] && [[ -s "${THETA_JAR_PATH}" ]]; then
    echo "Using cached Theta Terminal (download failed): ${THETA_JAR_PATH}" >&2
    return 0
  fi
  echo "Could not download ${THETA_JAR_URL} and no cache at ${THETA_JAR_PATH}" >&2
  exit 1
}

require_theta_creds() {
  local f="${THETA_JAR_DIR}/creds.txt"
  if [[ ! -f "${f}" ]] || [[ ! -s "${f}" ]]; then
    echo "Missing ${f} — two lines: email, then password (Theta Terminal --creds-file)." >&2
    exit 1
  fi
}

ensure_theta_image() {
  echo "Pulling Java image for Theta Terminal (${THETA_IMAGE})…"
  podman pull "${THETA_IMAGE}"
}

# If kube play runs while the Temurin pull would fail, Podman can leave a partial pod
verify_thetadata_container() {
  local pod_name="$1"
  sleep 1
  local n
  n="$(podman ps -a --filter "pod=${pod_name}" --format '{{.Names}}' 2>/dev/null | grep -c thetadata || true)"
  if [[ "${n}" -eq 0 ]]; then
    echo "ERROR: Pod '${pod_name}' has no *thetadata* container (broken or stale pod)." >&2
    echo "  Run: kube/play.sh down && podman pull ${THETA_IMAGE} && kube/play.sh ${2:-dev}" >&2
    exit 1
  fi
}

# ── Prod helpers ─────────────────────────────────────────────────────────────

detect_tls_domain() {
  local cert_dir="/etc/letsencrypt/live"
  for domain in $(ls "${cert_dir}" 2>/dev/null); do
    if [[ -f "${cert_dir}/${domain}/fullchain.pem" ]]; then
      echo "${domain}"
      return
    fi
  done
}

build_strategy_cmd() {
  local tls_domain="$1"
  local port="8080"
  [[ -n "${tls_domain}" ]] && port="443"
  local cmd="[\"./live\", \"--port\", \"${port}\""
  [[ -n "${DASHBOARD_AUTH:-}" ]] && cmd+=", \"--auth\", \"${DASHBOARD_AUTH}\""
  [[ -n "${tls_domain}" ]] && cmd+=", \"--tls-cert\", \"/certs/live/${tls_domain}/fullchain.pem\", \"--tls-key\", \"/certs/live/${tls_domain}/privkey.pem\""
  cmd+=']'
  echo "${cmd}"
}

# ── Wait helpers ─────────────────────────────────────────────────────────────

wait_for_gateway() {
  local host="${1:-localhost}" port="${2:-4002}" max=30
  echo "Waiting for IB Gateway on ${host}:${port}..."
  for _ in $(seq 1 $max); do
    nc -z "$host" "$port" 2>/dev/null && echo "IB Gateway is reachable on ${host}:${port}" && return 0
    sleep 2
  done
  echo "WARNING: IB Gateway not reachable on ${host}:${port} after ${max} attempts."
  echo "  - Is podman machine running?  podman machine start"
  echo "  - Check gateway logs:         podman logs <pod>-ib-gateway"
  echo "  - Verify port forwarding:     podman port <pod>"
  return 1
}

wait_for_thetadata() {
  local host="${1:-localhost}" port="${2:-25503}" max=45
  echo "Waiting for Theta Terminal HTTP on ${host}:${port}..."
  for _ in $(seq 1 $max); do
    nc -z "$host" "$port" 2>/dev/null && echo "Theta Terminal is reachable on ${host}:${port}" && return 0
    sleep 2
  done
  echo "WARNING: Theta Terminal not reachable on ${host}:${port} after ${max} attempts."
  echo "  - Logs:  kube/play.sh logs-theta"
  return 1
}

# ── Commands ─────────────────────────────────────────────────────────────────

usage() {
  cat <<EOF
Usage: kube/play.sh <command>

Theta Terminal: .thetadata/ (JAR + creds.txt only; Theta writes its own config dir).

Commands:
  build      Build the strategy container image
  prod       IB Gateway + Theta Terminal + strategy (all in one pod)
  dev        IB Gateway + Theta Terminal (run live/backtest via cargo on host)
  down       Tear down running dev/prod pods
  logs       Follow strategy container logs (prod)
  logs-theta Follow Theta Terminal container logs
  status     Check gateway + Theta ports on localhost
  gen        Print the dev manifest (dry run)
  gen-prod   Print the prod manifest
  ps         List gex-dev / gex-strategy pods and containers
EOF
  exit 1
}

case "${1:-}" in
  build)
    echo "Building strategy container image..."
    podman build -t gex-strategy -f "${PROJECT_DIR}/Containerfile" "${PROJECT_DIR}"
    echo ""
    echo "Image built: ${STRATEGY_IMAGE}"
    ;;
  dev)
    require_creds
    ensure_theta_jar
    require_theta_creds
    ensure_theta_image
    render dev.yaml.tpl | podman kube play --replace -
    verify_thetadata_container gex-dev dev
    wait_for_gateway
    wait_for_thetadata || true
    echo ""
    echo "Ready — Theta/Gateway on localhost; run e.g.:"
    echo "  cargo run --release --bin live"
    echo "  cargo run --release --bin backtest -- --ticker AAPL -p 8080"
    ;;
  prod)
    require_creds
    ensure_theta_jar
    require_theta_creds
    ensure_theta_image
    echo "Building strategy container image..."
    podman build -t gex-strategy -f "${PROJECT_DIR}/Containerfile" "${PROJECT_DIR}"
    echo ""
    tls_domain="$(detect_tls_domain)"
    export STRATEGY_CMD
    STRATEGY_CMD="$(build_strategy_cmd "${tls_domain}")"
    if [[ -n "${tls_domain}" ]]; then
      export DASH_PORT="443" DASH_SCHEME="HTTPS"
      render prod-tls.yaml.tpl | podman kube play --replace -
    else
      export DASH_PORT="8080" DASH_SCHEME="HTTP"
      render prod.yaml.tpl | podman kube play --replace -
    fi
    verify_thetadata_container gex-strategy prod
    wait_for_gateway
    wait_for_thetadata || true
    echo ""
    if [[ -n "${tls_domain}" ]]; then
      echo "Pod running — dashboard: https://${tls_domain}"
    else
      echo "Pod running — dashboard: http://localhost:8080"
    fi
    echo "Logs:  kube/play.sh logs | kube/play.sh logs-theta"
    echo "Stop:  kube/play.sh down"
    ;;
  down)
    podman pod rm -f gex-dev 2>/dev/null || true
    podman pod rm -f ib-gateway 2>/dev/null || true
    podman pod rm -f gex-strategy 2>/dev/null || true
    echo "Pod(s) stopped."
    ;;
  logs)
    podman logs -f gex-strategy-strategy
    ;;
  logs-theta)
    if podman inspect gex-strategy-thetadata &>/dev/null; then
      podman logs -f gex-strategy-thetadata
    elif podman inspect gex-dev-thetadata &>/dev/null; then
      podman logs -f gex-dev-thetadata
    else
      echo "No thetadata container found (start dev or prod first)." >&2
      exit 1
    fi
    ;;
  status)
    wait_for_gateway
    wait_for_thetadata || true
    ;;
  gen)
    require_creds
    render dev.yaml.tpl
    ;;
  gen-prod)
    require_creds
    tls_domain="$(detect_tls_domain)"
    export STRATEGY_CMD
    STRATEGY_CMD="$(build_strategy_cmd "${tls_domain}")"
    if [[ -n "${tls_domain}" ]]; then
      export DASH_PORT="443" DASH_SCHEME="HTTPS"
      render prod-tls.yaml.tpl
    else
      export DASH_PORT="8080" DASH_SCHEME="HTTP"
      render prod.yaml.tpl
    fi
    ;;
  ps)
    echo "== Pods =="
    podman pod ps --format 'table {{.Name}}\t{{.Status}}\t{{.InfraId}}'
    echo ""
    for p in gex-dev gex-strategy; do
      if podman pod exists "${p}" 2>/dev/null; then
        echo "== Containers (pod name filter: ${p}) =="
        podman ps -a --filter "pod=${p}" --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}'
        echo ""
      fi
    done
    ;;
  *)
    usage
    ;;
esac
