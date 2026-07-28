#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ocserv-docker-cleanup.sh  —  Stops and removes the OpenConnect test container
# Usage: bash ocserv-docker-cleanup.sh [PORT]
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VPN_PORT="${1:-4443}"
CONTAINER_NAME="ocserv-airlock-test"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Airlock — OpenConnect Test Server Cleanup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── Stop container ────────────────────────────────────────────────────────────
echo "[1/3] Stopping container: $CONTAINER_NAME"
if docker ps -q --filter "name=$CONTAINER_NAME" | grep -q .; then
    docker stop "$CONTAINER_NAME" && echo "  → Stopped"
else
    echo "  → Container not running (OK)"
fi

# ── Remove image (optional) ───────────────────────────────────────────────────
echo "[2/3] Container image..."
read -rp "  Remove Docker image too? (saves ~50MB) [y/N] " REMOVE_IMG
if [[ "${REMOVE_IMG:-N}" =~ ^[Yy]$ ]]; then
    docker rmi sctx/openconnect-server 2>/dev/null && echo "  → Image removed" || echo "  → Image not found (OK)"
else
    echo "  → Image kept (reuse without re-downloading)"
fi

# ── Close firewall ────────────────────────────────────────────────────────────
echo "[3/3] Closing firewall port $VPN_PORT..."
if command -v ufw &>/dev/null && sudo ufw status | grep -q "Status: active"; then
    sudo ufw delete allow "$VPN_PORT"/tcp >/dev/null 2>&1 && echo "  → Removed TCP rule" || echo "  → Rule not found (OK)"
    sudo ufw delete allow "$VPN_PORT"/udp >/dev/null 2>&1 && echo "  → Removed UDP rule" || echo "  → Rule not found (OK)"
else
    echo "  → UFW not active, skipping"
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ✅  Cleanup complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
