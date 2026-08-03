use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "windows")]
use base64::Engine as _;
#[cfg(target_os = "windows")]
use base64::engine::general_purpose::STANDARD as BASE64;
#[cfg(target_os = "windows")]
use boringtun::noise::{Tunn, TunnResult};
#[cfg(target_os = "windows")]
use boringtun::x25519::{StaticSecret, PublicKey};
#[cfg(target_os = "windows")]
use ipnetwork::IpNetwork;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

// ─── WireGuard Config Parser ──────────────────────────────────────────────────

#[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
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
            match section {
                "Interface" => { map.insert(k.trim(), v.trim()); }
                "Peer" => { peer_map.insert(k.trim(), v.trim()); }
                _ => {}
            }
        }
    }

    let private_key_b64 = map.get("PrivateKey").ok_or("Missing [Interface] PrivateKey")?;
    let private_key_bytes: [u8; 32] = BASE64
        .decode(private_key_b64)
        .map_err(|e| format!("Invalid PrivateKey base64: {e}"))?
        .try_into()
        .map_err(|_| "PrivateKey must be 32 bytes")?;
    let private_key = StaticSecret::from(private_key_bytes);

    let addr_str = map.get("Address").ok_or("Missing [Interface] Address")?;
    let address: IpNetwork = addr_str
        .split(',')
        .map(|s| s.trim())
        .find_map(|s| s.parse::<IpNetwork>().ok())
        .ok_or_else(|| format!("Invalid Address: {addr_str}"))?;

    let dns = map
        .get("DNS")
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok());

    let peer_pub_b64 = peer_map.get("PublicKey").ok_or("Missing [Peer] PublicKey")?;
    let peer_pub_bytes: [u8; 32] = BASE64
        .decode(peer_pub_b64)
        .map_err(|e| format!("Invalid peer PublicKey base64: {e}"))?
        .try_into()
        .map_err(|_| "Peer PublicKey must be 32 bytes")?;
    let peer_public_key = PublicKey::from(peer_pub_bytes);

    let endpoint_str = peer_map.get("Endpoint").ok_or("Missing [Peer] Endpoint")?;
    let peer_endpoint: SocketAddr = endpoint_str
        .parse()
        .map_err(|e| format!("Invalid Endpoint '{endpoint_str}': {e}"))?;

    let allowed_ips_str = peer_map.get("AllowedIPs").unwrap_or(&"0.0.0.0/0");
    let allowed_ips: Vec<IpNetwork> = allowed_ips_str
        .split(',')
        .filter_map(|s| s.trim().parse::<IpNetwork>().ok())
        .collect();

    let peer_preshared_key = peer_map
        .get("PresharedKey")
        .and_then(|s| BASE64.decode(s).ok())
        .and_then(|b| b.try_into().ok());

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

// ─── WireGuard Handle ─────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub struct VpnHandle {
    pub outbound_task: JoinHandle<()>,
    pub inbound_task: JoinHandle<()>,
    pub keepalive_task: Option<JoinHandle<()>>,
    #[allow(dead_code)]
    pub address: IpNetwork,
    pub allowed_ips: Vec<IpNetwork>,
    pub _adapter: Arc<wintun::Adapter>,
}

#[cfg(not(target_os = "windows"))]
pub struct VpnHandle {}

// ─── OpenConnect Handle ───────────────────────────────────────────────────────

pub struct OpenConnectHandle {
    /// The spawned openconnect.exe child process
    pub child: Arc<Mutex<tokio::process::Child>>,
    /// Background task monitoring stdout for connection events and MFA prompts
    pub status_task: JoinHandle<()>,
    /// Channel to send MFA tokens to the process stdin
    pub mfa_tx: mpsc::Sender<String>,
}

// ─── Active Tunnel (either WireGuard or OpenConnect) ─────────────────────────

pub enum ActiveTunnel {
    WireGuard(VpnHandle),
    OpenConnect(OpenConnectHandle),
}

// ─── VPN State ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct VpnState {
    pub tunnel: Arc<Mutex<Option<ActiveTunnel>>>,
}

impl VpnState {
    pub fn new() -> Self {
        VpnState {
            tunnel: Arc::new(Mutex::new(None)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VpnStatus {
    pub status: String,   // "connected" | "disconnected"
    pub protocol: String, // "wireguard" | "openconnect" | "none"
}

// ─── Tauri Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_vpn_status(state: tauri::State<'_, VpnState>) -> Result<VpnStatus, String> {
    let tunnel = state.tunnel.lock().await;
    Ok(match &*tunnel {
        None => VpnStatus { status: "disconnected".into(), protocol: "none".into() },
        Some(ActiveTunnel::WireGuard(_)) => VpnStatus { status: "connected".into(), protocol: "wireguard".into() },
        Some(ActiveTunnel::OpenConnect(_)) => VpnStatus { status: "connected".into(), protocol: "openconnect".into() },
    })
}

// ── WireGuard ─────────────────────────────────────────────────────────────────

#[tauri::command]
#[cfg(target_os = "windows")]
pub async fn start_vpn_tunnel(
    app: tauri::AppHandle,
    state: tauri::State<'_, VpnState>,
    config: String,
) -> Result<(), String> {
    use tauri::Emitter;

    {
        let t = state.tunnel.lock().await;
        if t.is_some() {
            return Err("A VPN tunnel is already active. Disconnect first.".into());
        }
    }

    let cfg = parse_wg_config(&config)?;

    // ── 1. Load wintun.dll ────────────────────────────────────────────────────
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

    // ── 2. Create Wintun adapter ─────────────────────────────────────────────
    let adapter = match wintun::Adapter::create(&wintun, "AirlockVPN", "WireGuard", None) {
        Ok(a) => a,
        Err(e) => {
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

    // ── 3. Assign IP + routes via netsh ──────────────────────────────────────
    let iface_ip = cfg.address.ip().to_string();
    let prefix_len = cfg.address.prefix();
    let netsh_ip = format!(
        "netsh interface ip set address name=\"AirlockVPN\" static {iface_ip} {} {} 1",
        ipnetwork_to_mask(&cfg.address),
        iface_ip
    );
    run_netsh(&netsh_ip)?;

    for net in &cfg.allowed_ips {
        if net.ip().to_string() == "0.0.0.0" && prefix_len == 0 {
            continue;
        }
        let route_cmd = format!(
            "netsh interface ip add route {}/{} \"AirlockVPN\"",
            net.ip(), net.prefix()
        );
        let _ = run_netsh(&route_cmd);
    }

    if let Some(dns_ip) = cfg.dns {
        let _ = run_netsh(&format!(
            "netsh interface ip set dns name=\"AirlockVPN\" static {}", dns_ip
        ));
    }

    // ── 4. Build boringtun tunnel ─────────────────────────────────────────────
    let tunn = Tunn::new(
        cfg.private_key.clone(),
        cfg.peer_public_key,
        cfg.peer_preshared_key,
        cfg.persistent_keepalive,
        0,
        None,
    );
    let tunn = Arc::new(std::sync::Mutex::new(tunn));

    // ── 5. UDP socket ─────────────────────────────────────────────────────────
    let udp = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Failed to bind UDP socket: {e}"))?;
    udp.connect(cfg.peer_endpoint)
        .map_err(|e| format!("Failed to connect UDP to peer: {e}"))?;
    udp.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| format!("set_read_timeout failed: {e}"))?;

    let udp_out = Arc::new(udp);
    let udp_in = udp_out.clone();

    // ── 6. Wintun session ─────────────────────────────────────────────────────
    let session = Arc::new(
        adapter
            .start_session(wintun::MAX_RING_CAPACITY)
            .map_err(|e| format!("Failed to start Wintun session: {e}"))?,
    );
    let session_out = session.clone();
    let session_in = session.clone();
    let tunn_out = tunn.clone();
    let tunn_in = tunn.clone();

    // ── 7. Outbound: TUN → encrypt → UDP ─────────────────────────────────────
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
                        TunnResult::WriteToNetwork(data) => { let _ = udp_out.send(data); }
                        TunnResult::Err(e) => { log::warn!("VPN encapsulate error: {:?}", e); }
                        _ => {}
                    }
                }
                Err(e) => { log::warn!("Wintun receive error: {}", e); break; }
            }
        }
    });

    // ── 8. Inbound: UDP → decrypt → TUN ──────────────────────────────────────
    let inbound_task = tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; 65535];
        let mut scratch = vec![0u8; 65535];
        let mut keepalive_scratch = vec![0u8; 148];

        loop {
            match udp_in.recv(&mut buf) {
                Ok(n) => {
                    let process = |result: TunnResult<'_>| {
                        match result {
                            TunnResult::WriteToTunnelV4(data, _) | TunnResult::WriteToTunnelV6(data, _) => {
                                if let Ok(mut pkt) = session_in.allocate_send_packet(data.len() as u16) {
                                    pkt.bytes_mut().copy_from_slice(data);
                                    session_in.send_packet(pkt);
                                }
                            }
                            TunnResult::WriteToNetwork(data) => { let _ = udp_in.send(data); }
                            TunnResult::Err(e) => { log::warn!("VPN decapsulate error: {:?}", e); }
                            _ => {}
                        }
                    };

                    {
                        let mut t = tunn_in.lock().unwrap();
                        let result = t.decapsulate(None, &buf[..n], &mut scratch);
                        process(result);
                    }

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
                    let mut t = tunn_in.lock().unwrap();
                    let result = t.update_timers(&mut keepalive_scratch);
                    drop(t);
                    if let TunnResult::WriteToNetwork(data) = result {
                        let _ = udp_in.send(data);
                    }
                }
                Err(e) => { log::warn!("VPN UDP recv error: {}", e); break; }
            }
        }
    });

    // ── 9. Store handle ───────────────────────────────────────────────────────
    {
        let mut tunnel = state.tunnel.lock().await;
        *tunnel = Some(ActiveTunnel::WireGuard(VpnHandle {
            outbound_task,
            inbound_task,
            keepalive_task: None,
            address: cfg.address,
            allowed_ips: cfg.allowed_ips,
            _adapter: adapter,
        }));
    }

    let _ = app.emit("vpn-status-changed", VpnStatus {
        status: "connected".into(),
        protocol: "wireguard".into(),
    });
    log::info!("WireGuard tunnel started successfully");
    Ok(())
}

#[tauri::command]
#[cfg(not(target_os = "windows"))]
pub async fn start_vpn_tunnel(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, VpnState>,
    _config: String,
) -> Result<(), String> {
    Err("WireGuard VPN is currently only supported on Windows in Airlock.".into())
}

// ── OpenConnect ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_openconnect_tunnel(
    app: tauri::AppHandle,
    state: tauri::State<'_, VpnState>,
    server: String,
    port: Option<u16>,
    servercert: Option<String>,  // pin-sha256:XXXX stored after user trusts the cert
    username: String,
    password: String,
    protocol_hint: String,
) -> Result<(), String> {
    use tauri::Emitter;

    {
        let t = state.tunnel.lock().await;
        if t.is_some() {
            return Err("A VPN tunnel is already active. Disconnect first.".into());
        }
    }

    if server.trim().is_empty() {
        return Err("Server URL cannot be empty. Please set a server address in the VPN profile.".into());
    }

    // ── 2. Locate openconnect.exe next to the binary ──────────────────────────
    let oc_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("openconnect.exe")))
        .unwrap_or_else(|| std::path::PathBuf::from("openconnect.exe"));

    if !oc_path.exists() {
        return Err(format!(
            "openconnect.exe not found at '{}'.\n\n\
             Download it from https://openconnect.github.io and place it next to the Airlock executable.",
            oc_path.display()
        ));
    }

    // ── 2. Build the command arguments ───────────────────────────────────────
    // Append port to server if non-standard
    let server_arg = match port {
        Some(p) if p != 443 => format!("{}:{}", server.trim_end_matches('/'), p),
        _ => server.clone(),
    };

    let mut args: Vec<String> = vec![
        server_arg.clone(),
        format!("--user={}", username),
        "--passwd-on-stdin".into(),
        "--non-inter".into(),
        "--timestamp".into(),
        "--reconnect-timeout=30".into(),
    ];

    // Trust a specific server certificate fingerprint (stored after user confirms)
    if let Some(ref cert) = servercert {
        let pin = if cert.starts_with("pin-sha256:") { cert.clone() }
                  else { format!("pin-sha256:{}", cert) };
        args.push(format!("--servercert={}", pin));
    }

    log::info!("[openconnect] Launching: {} {}", oc_path.display(), args.join(" "));

    // Map protocol hint to openconnect flag
    let protocol_flag = match protocol_hint.as_str() {
        "gp" | "globalprotect" => Some("gp"),
        "pulse" => Some("pulse"),
        "f5" => Some("f5"),
        "fortinet" => Some("fortinet"),
        "anyconnect" => Some("anyconnect"),
        _ => None, // auto-detect
    };
    if let Some(proto) = protocol_flag {
        args.push(format!("--protocol={}", proto));
    }
    // Report correct OS so the server selects the right client profile
    args.push("--os=win".into());
    // Verbose HTTP logging in debug builds so we can diagnose protocol issues
    #[cfg(debug_assertions)]
    args.push("--dump-http".into());

    // ── 3. Spawn the process with stdin/stdout piped ──────────────────────────
    // The MinGW-built openconnect looks for CA certs at a hardcoded Linux path.
    // We clear those env vars so OpenSSL doesn't try to stat a non-existent path.
    // Certificate verification for self-signed certs is handled via --servercert flag.
    let exe_dir = oc_path.parent().unwrap_or(&oc_path).to_path_buf();
    let mut cmd = tokio::process::Command::new(&oc_path);
    cmd.args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Clear the MinGW-hardcoded Linux CA paths to suppress "No such file" errors
        .env_remove("SSL_CERT_DIR")
        .env_remove("SSL_CERT_FILE")
        .env_remove("OPENSSL_DIR")
        // Ensure exe directory is first in PATH so bundled DLLs are found
        .env("PATH", format!("{};{}", exe_dir.display(),
            std::env::var("PATH").unwrap_or_default()));

    let mut child = cmd.spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                "Administrator privileges required to run openconnect.\n\n\
                 Please restart Airlock as Administrator.".into()
            } else {
                format!("Failed to launch openconnect.exe: {e}")
            }
        })?;

    // ── 4. Write password to stdin immediately ────────────────────────────────
    let mut stdin = child.stdin.take()
        .ok_or("Failed to open openconnect stdin")?;
    let pass_line = format!("{}\n", password);
    stdin.write_all(pass_line.as_bytes()).await
        .map_err(|e| format!("Failed to write password to openconnect: {e}"))?;

    // Keep stdin alive — we'll need it for 2FA token input
    let stdin = Arc::new(Mutex::new(stdin));
    let stdin_mfa = stdin.clone();

    // ── 5. MFA token channel ──────────────────────────────────────────────────
    let (mfa_tx, mut mfa_rx) = mpsc::channel::<String>(4);

    // ── 6. MFA stdin writer task ──────────────────────────────────────────────
    tokio::spawn(async move {
        while let Some(token) = mfa_rx.recv().await {
            let mut stdin_guard = stdin_mfa.lock().await;
            let _ = stdin_guard.write_all(format!("{}\n", token).as_bytes()).await;
        }
    });

    // ── 7. Stdout/stderr monitor task ─────────────────────────────────────────
    let stdout = child.stdout.take().ok_or("Failed to open openconnect stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to open openconnect stderr")?;
    let server_for_cert = server_arg.clone();

    let app_stdout = app.clone();
    let app_stderr = app.clone();
    // Clone for exit cleanup inside status_task
    let tunnel_arc_cleanup = state.tunnel.clone();
    let app_exit_cleanup = app.clone();

    // Monitor stdout
    let status_task = tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log::info!("[openconnect stdout] {}", line);
            let lower = line.to_lowercase();

            // Connection established
            if lower.contains("established dtls connection")
                || lower.contains("established tls connection")
                || lower.contains("vpn session established")
                || lower.contains("connected as ")
            {
                let _ = app_stdout.emit("vpn-status-changed", VpnStatus {
                    status: "connected".into(),
                    protocol: "openconnect".into(),
                });
                log::info!("OpenConnect tunnel established");
            }
            // MFA / 2FA prompt detected — be specific to avoid false-positives on HTTP logs
            // Real prompts contain "please enter", "enter your", or challenge keywords.
            // Exclude lines that start with a timestamp or contain "http response".
            else if !lower.contains("http response") && !lower.contains("http/1") && !lower.contains("password") && !lower.contains("username") && (
                lower.contains("passcode:")
                || lower.contains("challenge:")
                || lower.contains("otp:")
                || (lower.contains("token") && lower.contains("enter"))
            ) {
                log::info!("OpenConnect MFA prompt detected: {}", line.trim());
                let _ = app_stdout.emit("vpn-mfa-required", serde_json::json!({
                    "prompt": line.trim()
                }));
            }
            // Disconnected
            else if lower.contains("disconnected") || lower.contains("session terminated") {
                let _ = app_stdout.emit("vpn-status-changed", VpnStatus {
                    status: "disconnected".into(),
                    protocol: "none".into(),
                });
            }
            // Error
            else if lower.contains("failed to") || lower.contains("error:") || lower.contains("authentication failed") {
                let _ = app_stdout.emit("vpn-status-changed", serde_json::json!({
                    "status": "error",
                    "protocol": "openconnect",
                    "message": line.trim()
                }));
            }
        }
        // stdout pipe closed — the process has exited
        // Small sleep to ensure the tunnel handle was stored before we clear it
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        {
            let mut tunnel = tunnel_arc_cleanup.lock().await;
            *tunnel = None;
        }
        let _ = app_exit_cleanup.emit("vpn-status-changed", serde_json::json!({
            "status": "disconnected",
            "protocol": "none",
        }));
        log::info!("[openconnect] Process exited — tunnel state cleared");
    });

    // Monitor stderr (openconnect often logs to stderr)
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            log::warn!("[openconnect stderr] {}", line);
            let lower = line.to_lowercase();

            if lower.contains("established dtls connection")
                || lower.contains("established tls connection")
                || lower.contains("vpn session established")
                || lower.contains("connected as ")
            {
                let _ = app_stderr.emit("vpn-status-changed", VpnStatus {
                    status: "connected".into(),
                    protocol: "openconnect".into(),
                });
            } else if !lower.contains("http response") && !lower.contains("http/1") && !lower.contains("password") && !lower.contains("username") && (
                lower.contains("passcode:")
                || lower.contains("challenge:")
                || lower.contains("otp:")
                || (lower.contains("response:") && lower.trim_start().starts_with("response:"))
            ) {
                let _ = app_stderr.emit("vpn-mfa-required", serde_json::json!({
                    "prompt": line.trim()
                }));
            } else if lower.contains("certificate verify failed") || lower.contains("failed verification") {
                // Next line(s) will contain the --servercert hint
                log::warn!("[openconnect] Certificate verification failed, watching for fingerprint...");
            } else if lower.contains("pin-sha256:") {
                // Extract the fingerprint from openconnect's suggestion
                if let Some(start) = line.find("pin-sha256:") {
                    let fingerprint = line[start..].trim().to_string();
                    log::warn!("[openconnect] Cert fingerprint detected: {}", fingerprint);
                    let _ = app_stderr.emit("vpn-cert-verify", serde_json::json!({
                        "fingerprint": fingerprint,
                        "server": server_for_cert.clone(),
                    }));
                }
            } else if lower.contains("authentication failed") {
                let _ = app_stderr.emit("vpn-status-changed", serde_json::json!({
                    "status": "error",
                    "protocol": "openconnect",
                    "message": "Authentication failed — check username and password."
                }));
            }
        }
    });

    // ── 8. Store handle ───────────────────────────────────────────────────────
    {
        let mut tunnel = state.tunnel.lock().await;
        *tunnel = Some(ActiveTunnel::OpenConnect(OpenConnectHandle {
            child: Arc::new(Mutex::new(child)),
            status_task,
            mfa_tx,
        }));
    }

    // Emit "connecting" immediately — connected event comes from stdout monitor
    let _ = app.emit("vpn-status-changed", VpnStatus {
        status: "connecting".into(),
        protocol: "openconnect".into(),
    });
    log::info!("OpenConnect process spawned, waiting for connection...");
    Ok(())
}

/// Send a 2FA/MFA token to a running OpenConnect tunnel.
#[tauri::command]
pub async fn send_mfa_token(
    state: tauri::State<'_, VpnState>,
    token: String,
) -> Result<(), String> {
    let tunnel = state.tunnel.lock().await;
    match &*tunnel {
        Some(ActiveTunnel::OpenConnect(handle)) => {
            handle.mfa_tx.send(token).await
                .map_err(|_| "Failed to send MFA token — tunnel may have closed".into())
        }
        _ => Err("No active OpenConnect tunnel".into()),
    }
}

// ── Shared Disconnect ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn stop_vpn_tunnel(
    app: tauri::AppHandle,
    state: tauri::State<'_, VpnState>,
) -> Result<(), String> {
    use tauri::Emitter;

    let mut tunnel_guard = state.tunnel.lock().await;
    match tunnel_guard.take() {
        None => return Err("No active VPN tunnel".into()),

        #[cfg(target_os = "windows")]
        Some(ActiveTunnel::WireGuard(handle)) => {
            handle.outbound_task.abort();
            handle.inbound_task.abort();
            if let Some(k) = handle.keepalive_task { k.abort(); }

            for net in &handle.allowed_ips {
                if net.ip().to_string() == "0.0.0.0" { continue; }
                let _ = run_netsh(&format!(
                    "netsh interface ip delete route {}/{} \"AirlockVPN\"",
                    net.ip(), net.prefix()
                ));
            }
            drop(handle._adapter);
            log::info!("WireGuard tunnel stopped");
        }

        #[cfg(not(target_os = "windows"))]
        Some(ActiveTunnel::WireGuard(_handle)) => {}

        Some(ActiveTunnel::OpenConnect(handle)) => {
            // Abort the stdout monitor task
            handle.status_task.abort();

            // Kill the child process
            let mut child = handle.child.lock().await;
            let _ = child.kill().await;
            log::info!("OpenConnect process killed");
        }
    }

    let _ = app.emit("vpn-status-changed", VpnStatus {
        status: "disconnected".into(),
        protocol: "none".into(),
    });
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn run_netsh(cmd: &str) -> Result<(), String> {
    let output = std::process::Command::new("cmd")
        .args(["/C", cmd])
        .output()
        .map_err(|e| format!("Failed to run netsh command: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        log::warn!("netsh command failed: {cmd}\nstdout: {stdout}\nstderr: {stderr}");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn ipnetwork_to_mask(net: &IpNetwork) -> String {
    match net {
        IpNetwork::V4(n) => n.mask().to_string(),
        IpNetwork::V6(_) => net.prefix().to_string(),
    }
}
