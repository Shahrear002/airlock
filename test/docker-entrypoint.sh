#!/usr/bin/env bash
set -euo pipefail

VPN_USER="${VPN_USER:-testuser}"
VPN_PASS="${VPN_PASS:-testpass}"
VPN_PORT="${VPN_PORT:-4443}"
WORK_DIR="/etc/ocserv"
PUBLIC_IP="$(curl -s ifconfig.me 2>/dev/null || echo '0.0.0.0')"

echo "[ocserv] Starting OpenConnect test server"
echo "[ocserv] Server IP : $PUBLIC_IP"
echo "[ocserv] Port      : $VPN_PORT"
echo "[ocserv] User      : $VPN_USER"

# ── Generate self-signed certificate ─────────────────────────────────────────
if [ ! -f "$WORK_DIR/server.crt" ]; then
    echo "[ocserv] Generating self-signed certificate..."
    openssl req -x509 -newkey rsa:2048 \
        -keyout "$WORK_DIR/server.key" \
        -out "$WORK_DIR/server.crt" \
        -days 3650 -nodes \
        -subj "/CN=$PUBLIC_IP/O=Airlock-Test/C=US"
    chmod 600 "$WORK_DIR/server.key"
fi

# ── Create user ───────────────────────────────────────────────────────────────
echo "[ocserv] Creating user: $VPN_USER"
printf '%s\n%s\n' "$VPN_PASS" "$VPN_PASS" | ocpasswd -c "$WORK_DIR/ocpasswd" "$VPN_USER"

# ── Write config ──────────────────────────────────────────────────────────────
cat > "$WORK_DIR/ocserv.conf" << EOF
auth = "plain[passwd=$WORK_DIR/ocpasswd]"
tcp-port = $VPN_PORT
udp-port = $VPN_PORT
server-cert = $WORK_DIR/server.crt
server-key  = $WORK_DIR/server.key
ca-cert     = $WORK_DIR/server.crt
run-as-user  = root
run-as-group = root
isolate-workers = false
socket-file = /var/run/ocserv-socket
max-clients      = 10
max-same-clients = 5
keepalive    = 32400
dpd          = 90
auth-timeout = 120
device           = vpns
ipv4-network     = 192.168.200.0/24
dns              = 8.8.8.8
route            = default
cisco-client-compat = true
dtls-legacy         = true
tls-priorities = "NORMAL:%SERVER_PRECEDENCE:%COMPAT:-VERS-SSL3.0"
EOF

# ── Enable IP forwarding ──────────────────────────────────────────────────────
sysctl -w net.ipv4.ip_forward=1 >/dev/null 2>&1 || true

echo "[ocserv] Starting ocserv on port $VPN_PORT ..."
exec ocserv -c "$WORK_DIR/ocserv.conf" -f -d 1
