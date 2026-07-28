# VPN Connections

Airlock comes with built-in VPN support, giving you the ability to establish secure connections to corporate networks or personal privacy services directly from the app. This is perfect for accessing servers that are behind a VPN firewall without routing your entire OS traffic through it.

## Supported Protocols

Airlock supports two primary VPN protocols:

1. **WireGuard**: A modern, extremely fast and secure VPN protocol.
2. **OpenConnect**: An enterprise-grade SSL VPN client compatible with many major corporate VPN gateways.

### Which VPNs Will Work?

- **WireGuard**: Any VPN provider that allows you to download standard WireGuard configuration files (`.conf`), such as Mullvad, ProtonVPN, Surfshark, or your own self-hosted WireGuard servers (like PiVPN).
- **OpenConnect**: Most corporate gateways are supported, including:
  - Cisco AnyConnect
  - Juniper Network Connect
  - Pulse Connect Secure
  - Palo Alto GlobalProtect
  - Fortinet FortiGate (SSL VPN)
  - F5 BIG-IP

### Which VPNs Won't Work?

- **OpenVPN**: Legacy OpenVPN configurations (`.ovpn`) are not currently supported natively.
- **Proprietary Clients**: VPNs that require installing a highly specialized desktop client or device management profiles that cannot be exported to a standard configuration format.

---

## Adding a WireGuard Profile

WireGuard profiles are configured using standard configuration files.

1. Click on the **VPN** tab located in the left sidebar next to the Explorer tab.
2. Click the **+ Profile** button at the bottom of the sidebar.
3. Select **WireGuard** as the protocol.
4. Fill in the required details:
   - **Profile Name**: A descriptive name (e.g., `Mullvad US`, `Home Lab`).
   - **WireGuard Configuration**: Paste the entire contents of your `.conf` file into the large text area. This includes your `[Interface]` (PrivateKey, Address) and `[Peer]` (PublicKey, Endpoint, AllowedIPs) blocks.
5. Click **Save profile**.

## Adding an OpenConnect Profile

OpenConnect profiles are configured using a server URL and your credentials.

1. Click on the **VPN** tab in the sidebar.
2. Click the **+ Profile** button.
3. Select **OpenConnect** as the protocol.
4. Fill in the required details:
   - **Profile Name**: A descriptive name (e.g., `Corporate VPN`).
   - **Server URL**: The hostname or IP address of the VPN gateway (e.g., `vpn.company.com`).
   - **Port**: The port number, usually `443`.
   - **Server Protocol**: Select your corporate gateway type (Cisco, GlobalProtect, etc.). If you are unsure, choose **Auto-detect** and Airlock will probe the server.
   - **Username/Password**: Your credentials for the VPN server.
5. Click **Save profile**.

---

## Multi-Factor Authentication (MFA)

If your OpenConnect VPN provider requires a One-Time Password (OTP) or Multi-Factor Authentication token, Airlock seamlessly handles it. 

When you connect to the profile, if the server requests an MFA token, a dialog will automatically prompt you to enter it. You do not need to configure MFA keys beforehand in the profile settings.

## Managing Connections

- **Connect/Disconnect**: Click the connection toggle switch or the **Connect** button on the profile card.
- **Edit/Delete**: Click the pencil icon to edit, or the trash can icon to delete the profile.

*Note: For OpenConnect connections, if the server uses a self-signed certificate, you will be prompted to trust the certificate fingerprint upon your first connection attempt.*
