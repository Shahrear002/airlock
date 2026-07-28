#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ocserv-cleanup.sh  —  Removes the ocserv test server and all its files
# Usage: bash ocserv-cleanup.sh
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VPN_PORT="${1:-4443}"
WORK_DIR="$HOME/ocserv-test"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Airlock — OpenConnect Test Server Cleanup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── 1. Kill any running ocserv process ────────────────────────────────────────
echo "[1/4] Stopping ocserv..."
if [ -f /tmp/ocserv-test.pid ]; then
    sudo kill "$(cat /tmp/ocserv-test.pid)" 2>/dev/null && echo "  → Stopped via PID file" || true
    sudo rm -f /tmp/ocserv-test.pid
fi
# Also kill by name in case PID file is missing
sudo pkill -f "ocserv -c $WORK_DIR" 2>/dev/null && echo "  → Killed by name" || echo "  → No running ocserv found (OK)"

# ── 2. Remove VPN network interface if it exists ─────────────────────────────
echo "[2/4] Removing VPN network interface..."
if ip link show vpns0 &>/dev/null; then
    sudo ip link delete vpns0 2>/dev/null && echo "  → Removed vpns0" || true
fi
for iface in $(ip link show | grep -oE 'vpns[0-9]+' 2>/dev/null || true); do
    sudo ip link delete "$iface" 2>/dev/null && echo "  → Removed $iface" || true
done

# ── 3. Close firewall ports ───────────────────────────────────────────────────
echo "[3/4] Closing firewall port $VPN_PORT..."
if command -v ufw &>/dev/null; then
    sudo ufw delete allow "$VPN_PORT"/tcp >/dev/null 2>&1 && echo "  → Removed UFW TCP rule" || echo "  → TCP rule not found (OK)"
    sudo ufw delete allow "$VPN_PORT"/udp >/dev/null 2>&1 && echo "  → Removed UFW UDP rule" || echo "  → UDP rule not found (OK)"
fi

# ── 4. Remove working directory ───────────────────────────────────────────────
echo "[4/4] Removing work directory: $WORK_DIR"
if [ -d "$WORK_DIR" ]; then
    rm -rf "$WORK_DIR"
    echo "  → Removed $WORK_DIR"
else
    echo "  → Directory not found (OK)"
fi

# ── Done ─────────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ✅  Cleanup complete! All ocserv test files removed."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
