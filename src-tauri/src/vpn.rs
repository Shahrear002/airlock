use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{StaticSecret, PublicKey};
use ipnetwork::IpNetwork;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

// ─── Config Parser ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct WgConfig {
    pub private_key: StaticSecret,
    pub address: IpNetwork,
    pub dns: Option<IpAddr>,
    pub peer_public_key: PublicKey,
    pub peer_endpoint: SocketAddr,
    pub allowed_ips: Vec<IpNetwork>,
    pub peer_preshared_key: Option<[u8; 32]>,
    pub persistent_keepalive: Option<u16>,
}

/// Parses a WireGuard .conf file (INI-like format) into a WgConfig struct.
pub fn parse_wg_config(raw: &str) -> Result<WgConfig, String> {
    let mut section = "";
    let mut map: HashMap<&str, &str> = HashMap::new();
    let mut peer_map: HashMap<&str, &str> = HashMap::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            match section {
                "Interface" => { map.insert(key, val); }
                "Peer" => { peer_map.insert(key, val); }
                _ => {}
            }
        }
    }

    // Parse private key
    let private_key_b64 = map.get("PrivateKey").ok_or("Missing [Interface] PrivateKey")?;
    let private_key_bytes: [u8; 32] = BASE64
        .decode(private_key_b64)
        .map_err(|e| format!("Invalid PrivateKey base64: {e}"))?
        .try_into()
        .map_err(|_| "PrivateKey must be 32 bytes")?;
    let private_key = StaticSecret::from(private_key_bytes);

    // Parse interface address
    let addr_str = map.get("Address").ok_or("Missing [Interface] Address")?;
    // Support comma-separated list; take the first IPv4 one
    let address: IpNetwork = addr_str
        .split(',')
        .map(|s| s.trim())
        .find_map(|s| s.parse::<IpNetwork>().ok())
        .ok_or_else(|| format!("Invalid Address: {addr_str}"))?;

    // Parse optional DNS
    let dns = map
        .get("DNS")
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok());

    // Parse peer public key
    let peer_pub_b64 = peer_map.get("PublicKey").ok_or("Missing [Peer] PublicKey")?;
    let peer_pub_bytes: [u8; 32] = BASE64
        .decode(peer_pub_b64)
        .map_err(|e| format!("Invalid peer PublicKey base64: {e}"))?
        .try_into()
        .map_err(|_| "Peer PublicKey must be 32 bytes")?;
    let peer_public_key = PublicKey::from(peer_pub_bytes);

    // Parse peer endpoint
    let endpoint_str = peer_map.get("Endpoint").ok_or("Missing [Peer] Endpoint")?;
    let peer_endpoint: SocketAddr = endpoint_str
        .parse()
        .map_err(|e| format!("Invalid Endpoint '{endpoint_str}': {e}"))?;

    // Parse allowed IPs
    let allowed_ips_str = peer_map.get("AllowedIPs").unwrap_or(&"0.0.0.0/0");
    let allowed_ips: Vec<IpNetwork> = allowed_ips_str
        .split(',')
        .filter_map(|s| s.trim().parse::<IpNetwork>().ok())
        .collect();

    // Parse optional preshared key
    let peer_preshared_key = peer_map
        .get("PresharedKey")
        .and_then(|s| BASE64.decode(s).ok())
        .and_then(|b| b.try_into().ok());

    // Parse optional persistent keepalive
    let persistent_keepalive = peer_map
        .get("PersistentKeepalive")
        .and_then(|s| s.parse::<u16>().ok());

    Ok(WgConfig {
        private_key,
        address,
        dns,
        peer_public_key,
        peer_endpoint,
        allowed_ips,
        peer_preshared_key,
        persistent_keepalive,
    })
}

// ─── VPN State ───────────────────────────────────────────────────────────────

/// Holds runtime handles for a running VPN tunnel.
pub struct VpnHandle {
    pub outbound_task: JoinHandle<()>,
    pub inbound_task: JoinHandle<()>,
    pub keepalive_task: Option<JoinHandle<()>>,
    /// Interface IP for cleanup via netsh (reserved for future use)
    #[allow(dead_code)]
    pub address: IpNetwork,
    /// Allowed IPs for route cleanup
    pub allowed_ips: Vec<IpNetwork>,
    /// The wintun adapter — kept alive here; dropping it tears down the NIC.
    pub _adapter: Arc<wintun::Adapter>,
}

#[derive(Clone)]
pub struct VpnState {
    pub handle: Arc<Mutex<Option<VpnHandle>>>,
}

impl VpnState {
    pub fn new() -> Self {
        VpnState {
            handle: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VpnStatus {
    pub status: String, // "connected" | "disconnected"
}

// ─── Tauri Commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_vpn_status(state: tauri::State<'_, VpnState>) -> Result<VpnStatus, String> {
    let handle = state.handle.lock().await;
    Ok(VpnStatus {
        status: if handle.is_some() {
            "connected".into()
        } else {
            "disconnected".into()
        },
    })
}

#[tauri::command]
pub async fn start_vpn_tunnel(
    app: tauri::AppHandle,
    state: tauri::State<'_, VpnState>,
    config: String,
) -> Result<(), String> {
    use tauri::Emitter;

    // Prevent double-connect
    {
        let h = state.handle.lock().await;
        if h.is_some() {
            return Err("VPN tunnel is already active".into());
        }
    }

    let cfg = parse_wg_config(&config)?;

    // ── 1. Load wintun.dll ─────────────────────────────────────────
    // Look for wintun.dll next to the binary first, then fallback to PATH
    let wintun_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("wintun.dll")))
        .unwrap_or_else(|| std::path::PathBuf::from("wintun.dll"));

    let wintun = unsafe {
        wintun::load_from_path(&wintun_path).map_err(|e| {
            format!(
                "Failed to load wintun.dll from '{}': {}\n\nMake sure wintun.dll is in the same folder as the Airlock executable.",
                wintun_path.display(), e
            )
        })?
    };

    // ── 2. Create or open the Wintun adapter ─────────────────────────────────
    let adapter = match wintun::Adapter::create(&wintun, "AirlockVPN", "WireGuard", None) {
        Ok(a) => a, // create() returns Arc<Adapter> directly
        Err(e) => {
            // ERROR_ACCESS_DENIED (5) means we need admin privileges
            let msg = e.to_string();
            let is_access_denied = msg.contains("5") || msg.to_lowercase().contains("access");
            if is_access_denied {
                return Err(
                    "Administrator privileges required to create VPN adapter.\n\n\
                     Please restart Airlock by right-clicking the executable and selecting \"Run as administrator\", \
                     then try connecting to VPN again.".into()
                );
            }
            return Err(format!("Failed to create WireGuard adapter: {e}"));
        }
    };

    // ── 3. Assign IP address + routes via netsh ───────────────────────────────
    let iface_ip = cfg.address.ip().to_string();
    let prefix_len = cfg.address.prefix();
    let netsh_ip = format!(
        "netsh interface ip set address name=\"AirlockVPN\" static {iface_ip} {} {} 1",
        ipnetwork_to_mask(&cfg.address),
        iface_ip // gateway = self for TUN
    );
    run_netsh(&netsh_ip)?;

    // Add routes for AllowedIPs
    for net in &cfg.allowed_ips {
        if net.ip().to_string() == "0.0.0.0" && prefix_len == 0 {
            // Default route — skip to avoid breaking connectivity; handled at OS level
            continue;
        }
        let route_cmd = format!(
            "netsh interface ip add route {}/{} \"AirlockVPN\"",
            net.ip(),
            net.prefix()
        );
        let _ = run_netsh(&route_cmd); // Best-effort; routes may already exist
    }

    // Optionally set DNS
    if let Some(dns_ip) = cfg.dns {
        let dns_cmd = format!(
            "netsh interface ip set dns name=\"AirlockVPN\" static {}",
            dns_ip
        );
        let _ = run_netsh(&dns_cmd);
    }

    // ── 4. Build the boringtun tunnel ─────────────────────────────────────────
    // boringtun 0.7: Tunn::new is infallible (returns Tunn directly)
    let tunn = Tunn::new(
        cfg.private_key.clone(),
        cfg.peer_public_key,
        cfg.peer_preshared_key,
        cfg.persistent_keepalive,
        0, // index
        None,
    );
    let tunn = Arc::new(std::sync::Mutex::new(tunn));

    // ── 5. Open UDP socket ────────────────────────────────────────────────────
    let udp = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind UDP socket: {e}"))?;
    udp.connect(cfg.peer_endpoint)
        .map_err(|e| format!("Failed to connect UDP to peer: {e}"))?;
    udp.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| format!("set_read_timeout failed: {e}"))?;

    let udp_out = Arc::new(udp);
    let udp_in = udp_out.clone();

    // ── 6. Start Wintun session ────────────────────────────────────────────────
    let session = Arc::new(
        adapter
            .start_session(wintun::MAX_RING_CAPACITY)
            .map_err(|e| format!("Failed to start Wintun session: {e}"))?,
    );
    let session_out = session.clone();
    let session_in = session.clone();

    let tunn_out = tunn.clone();
    let tunn_in = tunn.clone();

    // ── 7. Outbound task: TUN → boringtun.encapsulate → UDP ──────────────────
    let outbound_task = tokio::task::spawn_blocking(move || {
        let mut scratch = vec![0u8; 65535];
        loop {
            match session_out.receive_blocking() {
                Ok(pkt) => {
                    let result = {
                        let mut t = tunn_out.lock().unwrap();
                        t.encapsulate(pkt.bytes(), &mut scratch)
                    };
                    match result {
                        TunnResult::WriteToNetwork(data) => {
                            let _ = udp_out.send(data);
                        }
                        TunnResult::Done => {}
                        TunnResult::Err(e) => {
                            log::warn!("VPN encapsulate error: {:?}", e);
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    log::warn!("Wintun receive error: {}", e);
                    break;
                }
            }
        }
    });

    // ── 8. Inbound task: UDP → boringtun.decapsulate → TUN ───────────────────
    let inbound_task = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; 65535];
        let mut scratch = vec![0u8; 65535];
        let mut keepalive_scratch = vec![0u8; 148];

        loop {
            match udp_in.recv(&mut buf) {
                Ok(n) => {
                    // Process the first decapsulate result immediately (no Vec)
                    let process = |result: TunnResult<'_>| {
                        match result {
                            TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                                if let Ok(mut pkt) = session_in.allocate_send_packet(data.len() as u16) {
                                    pkt.bytes_mut().copy_from_slice(data);
                                    session_in.send_packet(pkt);
                                }
                            }
                            TunnResult::WriteToNetwork(data) => {
                                let _ = udp_in.send(data);
                            }
                            TunnResult::Done => {}
                            TunnResult::Err(e) => {
                                log::warn!("VPN decapsulate error: {:?}", e);
                            }
                        }
                    };

                    // Decapsulate first packet
                    {
                        let mut t = tunn_in.lock().unwrap();
                        let result = t.decapsulate(None, &buf[..n], &mut scratch);
                        process(result);
                    }

                    // Drain any additional queued outbound packets
                    loop {
                        let mut t = tunn_in.lock().unwrap();
                        let result = t.decapsulate(None, &[], &mut scratch);
                        let done = matches!(result, TunnResult::Done);
                        process(result);
                        if done { break; }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Timeout — drive timers and send keepalive if needed
                    let mut t = tunn_in.lock().unwrap();
                    let result = t.update_timers(&mut keepalive_scratch);
                    drop(t);
                    if let TunnResult::WriteToNetwork(data) = result {
                        let _ = udp_in.send(data);
                    }
                }
                Err(e) => {
                    log::warn!("VPN UDP recv error: {}", e);
                    break;
                }
            }
        }
    });

    // ── 9. Optional explicit keepalive task ──────────────────────────────────
    let keepalive_task = None; // Handled in the inbound loop via update_timers

    // ── 10. Store handle ─────────────────────────────────────────────────────
    {
        let mut h = state.handle.lock().await;
        *h = Some(VpnHandle {
            outbound_task,
            inbound_task,
            keepalive_task,
            address: cfg.address,
            allowed_ips: cfg.allowed_ips,
            _adapter: adapter,
        });
    }

    let _ = app.emit("vpn-status-changed", VpnStatus { status: "connected".into() });
    log::info!("VPN tunnel started successfully");
    Ok(())
}

#[tauri::command]
pub async fn stop_vpn_tunnel(
    app: tauri::AppHandle,
    state: tauri::State<'_, VpnState>,
) -> Result<(), String> {
    use tauri::Emitter;

    let mut h_guard = state.handle.lock().await;
    if let Some(handle) = h_guard.take() {
        // Abort background tasks
        handle.outbound_task.abort();
        handle.inbound_task.abort();
        if let Some(k) = handle.keepalive_task {
            k.abort();
        }

        // Remove routes
        for net in &handle.allowed_ips {
            if net.ip().to_string() == "0.0.0.0" {
                continue;
            }
            let cmd = format!(
                "netsh interface ip delete route {}/{} \"AirlockVPN\"",
                net.ip(),
                net.prefix()
            );
            let _ = run_netsh(&cmd);
        }

        // Dropping _adapter cleans up the Wintun NIC
        drop(handle._adapter);

        let _ = app.emit("vpn-status-changed", VpnStatus { status: "disconnected".into() });
        log::info!("VPN tunnel stopped");
    } else {
        return Err("No active VPN tunnel".into());
    }

    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn run_netsh(cmd: &str) -> Result<(), String> {
    let output = std::process::Command::new("cmd")
        .args(["/C", cmd])
        .output()
        .map_err(|e| format!("Failed to run netsh command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        log::warn!("netsh command failed: {cmd}\nstdout: {stdout}\nstderr: {stderr}");
        // Don't hard-fail — some netsh errors are benign (e.g., route already exists)
    }
    Ok(())
}

fn ipnetwork_to_mask(net: &IpNetwork) -> String {
    match net {
        IpNetwork::V4(n) => n.mask().to_string(),
        IpNetwork::V6(_) => net.prefix().to_string(),
    }
}
