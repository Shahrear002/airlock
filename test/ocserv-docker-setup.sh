#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ocserv-docker-setup.sh  —  Builds and runs an OpenConnect test server
# Usage: bash ocserv-docker-setup.sh [USERNAME] [PASSWORD] [PORT]
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VPN_USER="${1:-testuser}"
VPN_PASS="${2:-testpass}"
VPN_PORT="${3:-4443}"
CONTAINER_NAME="ocserv-airlock-test"
IMAGE_NAME="airlock-ocserv"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLIC_IP="$(curl -s ifconfig.me 2>/dev/null || curl -s api.ipify.org 2>/dev/null || echo '127.0.0.1')"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Airlock — OpenConnect Test Server (Docker)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  VPS IP   : $PUBLIC_IP"
echo "  Port     : $VPN_PORT"
echo "  Username : $VPN_USER"
echo "  Password : $VPN_PASS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── Check Docker ──────────────────────────────────────────────────────────────
if ! command -v docker &>/dev/null; then
    echo "❌ Docker not found. Install with: curl -fsSL https://get.docker.com | bash"
    exit 1
fi
if ! docker info &>/dev/null; then
    echo "❌ Docker daemon not running: sudo systemctl start docker"
    exit 1
fi

# ── Stop existing container ───────────────────────────────────────────────────
if docker ps -q --filter "name=$CONTAINER_NAME" | grep -q .; then
    echo "⚠  Stopping existing container..."
    docker stop "$CONTAINER_NAME" >/dev/null
fi

# ── Build image from local Dockerfile ────────────────────────────────────────
echo "[1/3] Building ocserv Docker image..."
docker build -t "$IMAGE_NAME" "$SCRIPT_DIR" --quiet
echo "  → Image built: $IMAGE_NAME"

# ── Open firewall ─────────────────────────────────────────────────────────────
if command -v ufw &>/dev/null && sudo ufw status 2>/dev/null | grep -q "Status: active"; then
    echo "[2/3] Opening firewall port $VPN_PORT..."
    sudo ufw allow "$VPN_PORT"/tcp >/dev/null 2>&1 || true
    sudo ufw allow "$VPN_PORT"/udp >/dev/null 2>&1 || true
else
    echo "[2/3] Firewall: skipped (ufw not active)"
fi

# ── Start container ───────────────────────────────────────────────────────────
echo "[3/3] Starting OpenConnect container..."
docker run -d \
    --name "$CONTAINER_NAME" \
    --rm \
    --privileged \
    --cap-add NET_ADMIN \
    --cap-add SYS_PTRACE \
    -p "$VPN_PORT:$VPN_PORT/tcp" \
    -p "$VPN_PORT:$VPN_PORT/udp" \
    -e "VPN_USER=$VPN_USER" \
    -e "VPN_PASS=$VPN_PASS" \
    -e "VPN_PORT=$VPN_PORT" \
    "$IMAGE_NAME"

echo "  Waiting for server to initialize..."
sleep 5

# ── Check status ──────────────────────────────────────────────────────────────
if docker ps --filter "name=$CONTAINER_NAME" --format "{{.Status}}" | grep -q "Up"; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  ✅  OpenConnect server is running!"
    echo ""
    echo "  ── Airlock Profile Settings ──────────────────"
    echo "  Server   : $PUBLIC_IP"
    echo "  Port     : $VPN_PORT"
    echo "  Username : $VPN_USER"
    echo "  Password : $VPN_PASS"
    echo "  Protocol : Auto"
    echo ""
    echo "  ── Useful Commands ───────────────────────────"
    echo "  Logs   : docker logs -f $CONTAINER_NAME"
    echo "  Stop   : bash ocserv-docker-cleanup.sh"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  💡 The server has a self-signed cert — the"
    echo "     'Trust Certificate' dialog will appear"
    echo "     on first connect in Airlock. Click Trust!"
else
    echo ""
    echo "❌ Container failed. Check logs: docker logs $CONTAINER_NAME"
    docker logs "$CONTAINER_NAME" 2>&1 | tail -20
    exit 1
fi
