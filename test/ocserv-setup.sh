#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ocserv-setup.sh  —  Sets up a local OpenConnect test server (ocserv)
# Usage: bash ocserv-setup.sh [VPN_USERNAME] [VPN_PASSWORD] [PORT]
# Defaults: username=testuser, password=testpass, port=4443
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

VPN_USER="${1:-testuser}"
VPN_PASS="${2:-testpass}"
VPN_PORT="${3:-4443}"
WORK_DIR="$HOME/ocserv-test"
PUBLIC_IP="$(curl -s ifconfig.me || curl -s api.ipify.org || echo '127.0.0.1')"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Airlock — OpenConnect Test Server Setup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  VPS IP   : $PUBLIC_IP"
echo "  Port     : $VPN_PORT"
echo "  Username : $VPN_USER"
echo "  Password : $VPN_PASS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── 1. Install dependencies ───────────────────────────────────────────────────
echo "[1/5] Installing ocserv..."
sudo apt-get update -q
sudo apt-get install -y -q ocserv openssl curl

# ── 2. Create working directory ───────────────────────────────────────────────
echo "[2/5] Creating work directory: $WORK_DIR"
mkdir -p "$WORK_DIR"
cd "$WORK_DIR"

# ── 3. Generate self-signed certificate ───────────────────────────────────────
echo "[3/5] Generating self-signed TLS certificate..."
openssl req -x509 -newkey rsa:2048 -keyout server.key -out server.crt \
  -days 365 -nodes \
  -subj "/CN=$PUBLIC_IP/O=Airlock-Test/C=US" \
  -addext "subjectAltName=IP:$PUBLIC_IP,IP:127.0.0.1"

chmod 600 server.key

# ── 4. Write ocserv config ────────────────────────────────────────────────────
echo "[4/5] Writing ocserv configuration..."
cat > "$WORK_DIR/ocserv.conf" << EOF
# ── Authentication ────────────────────────────────────────────────────────────
auth = "plain[passwd=$WORK_DIR/ocpasswd]"

# ── Network ───────────────────────────────────────────────────────────────────
tcp-port = $VPN_PORT
udp-port = $VPN_PORT

# ── Certificate ───────────────────────────────────────────────────────────────
server-cert = $WORK_DIR/server.crt
server-key  = $WORK_DIR/server.key
ca-cert     = $WORK_DIR/server.crt

# ── Process ───────────────────────────────────────────────────────────────────
run-as-user  = nobody
run-as-group = nogroup
isolate-workers = false
pid-file = /tmp/ocserv-test.pid
socket-file = /tmp/ocserv-socket

# ── Limits ────────────────────────────────────────────────────────────────────
max-clients      = 10
max-same-clients = 5

# ── Timeouts ─────────────────────────────────────────────────────────────────
keepalive   = 32400
dpd         = 90
auth-timeout = 120

# ── VPN Network ───────────────────────────────────────────────────────────────
device           = vpns
ipv4-network     = 192.168.200.0/24
dns              = 8.8.8.8
route            = default

# ── Compatibility ─────────────────────────────────────────────────────────────
cisco-client-compat = true
dtls-legacy         = true
tls-priorities = "NORMAL:%SERVER_PRECEDENCE:%COMPAT:-VERS-SSL3.0"
EOF

# ── 5. Create test user ───────────────────────────────────────────────────────
echo "[5/5] Creating VPN user: $VPN_USER"
# Non-interactively create the user in ocpasswd format
echo "$VPN_PASS" | sudo ocpasswd -c "$WORK_DIR/ocpasswd" "$VPN_USER"

# ── Open firewall ─────────────────────────────────────────────────────────────
if command -v ufw &>/dev/null; then
    echo ""
    echo "[UFW] Opening port $VPN_PORT (TCP + UDP)..."
    sudo ufw allow "$VPN_PORT"/tcp >/dev/null 2>&1 || true
    sudo ufw allow "$VPN_PORT"/udp >/dev/null 2>&1 || true
fi

# ── Enable IP forwarding ──────────────────────────────────────────────────────
sudo sysctl -w net.ipv4.ip_forward=1 >/dev/null

# ── Done ─────────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ✅  Setup complete!"
echo ""
echo "  Start the server with:"
echo "    sudo ocserv -c $WORK_DIR/ocserv.conf -f -d 1"
echo ""
echo "  In Airlock, create a new OpenConnect profile:"
echo "    Server   : $PUBLIC_IP"
echo "    Port     : $VPN_PORT"
echo "    Username : $VPN_USER"
echo "    Password : $VPN_PASS"
echo "    Protocol : Auto"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── Start ocserv now ─────────────────────────────────────────────────────────
read -rp "Start ocserv now? [Y/n] " START
if [[ "${START:-Y}" =~ ^[Yy]$ ]]; then
    echo ""
    echo "  Starting ocserv on port $VPN_PORT ... (Ctrl+C to stop)"
    echo ""
    sudo ocserv -c "$WORK_DIR/ocserv.conf" -f -d 1
fi
