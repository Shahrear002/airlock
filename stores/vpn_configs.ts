import { defineStore } from 'pinia'
import { ref } from 'vue'
import { encrypt, decrypt } from '~/utils/security'
import { TauriStoreAdapter } from '~/utils/store-adapter'

export type VpnProtocol = 'wireguard' | 'openconnect'

export type OpenConnectProtocolHint =
    | 'auto'
    | 'anyconnect'
    | 'gp'        // Palo Alto GlobalProtect
    | 'pulse'     // Pulse Secure / Ivanti
    | 'f5'        // F5 BIG-IP
    | 'fortinet'  // Fortinet FortiGate

export interface VpnProfile {
    id: string
    name: string
    protocol: VpnProtocol

    // ── WireGuard fields ───────────────────────────────────────────────────────
    encrypted_config?: string          // AES-256-GCM encrypted .conf content

    // ── OpenConnect fields ─────────────────────────────────────────────────────
    server_url?: string                // e.g. vpn.company.com or https://vpn.example.com
    port?: number                      // default 443, customisable for non-standard deployments
    username?: string                  // stored plaintext (not sensitive on its own)
    encrypted_password?: string        // AES-256-GCM encrypted password
    protocol_hint?: OpenConnectProtocolHint  // maps to --protocol= flag
    servercert?: string                // pin-sha256:XXXX — set after user trusts self-signed cert
}

export const useVpnConfigsStore = defineStore('vpn_configs', () => {
    const profiles = ref<VpnProfile[]>([])

    // ── WireGuard ──────────────────────────────────────────────────────────────

    async function addWireGuardProfile(name: string, rawConfig: string): Promise<void> {
        const id = crypto.randomUUID()
        const encrypted_config = await encrypt(rawConfig)
        profiles.value.push({ id, name, protocol: 'wireguard', encrypted_config })
    }

    async function updateWireGuardProfile(id: string, name: string, rawConfig: string): Promise<void> {
        const idx = profiles.value.findIndex(p => p.id === id)
        if (idx === -1) return
        const encrypted_config = rawConfig
            ? await encrypt(rawConfig)
            : profiles.value[idx].encrypted_config
        profiles.value[idx] = { ...profiles.value[idx], name, encrypted_config }
    }

    async function getDecryptedConfig(id: string): Promise<string> {
        const profile = profiles.value.find(p => p.id === id)
        if (!profile?.encrypted_config) return ''
        return await decrypt(profile.encrypted_config)
    }

    // ── OpenConnect ────────────────────────────────────────────────────────────

    async function addOpenConnectProfile(
        name: string,
        server_url: string,
        port: number | undefined,
        username: string,
        password: string,
        protocol_hint: OpenConnectProtocolHint = 'auto',
    ): Promise<void> {
        const id = crypto.randomUUID()
        const encrypted_password = password ? await encrypt(password) : ''
        profiles.value.push({
            id,
            name,
            protocol: 'openconnect',
            server_url,
            port: port || undefined,
            username,
            encrypted_password,
            protocol_hint,
        })
    }

    async function updateOpenConnectProfile(
        id: string,
        name: string,
        server_url: string,
        port: number | undefined,
        username: string,
        password: string,
        protocol_hint: OpenConnectProtocolHint = 'auto',
    ): Promise<void> {
        const idx = profiles.value.findIndex(p => p.id === id)
        if (idx === -1) return
        const encrypted_password = password
            ? await encrypt(password)
            : profiles.value[idx].encrypted_password ?? ''
        profiles.value[idx] = {
            ...profiles.value[idx],
            name,
            server_url,
            port: port || undefined,
            username,
            encrypted_password,
            protocol_hint,
        }
    }

    async function getDecryptedPassword(id: string): Promise<string> {
        const profile = profiles.value.find(p => p.id === id)
        if (!profile?.encrypted_password) return ''
        return await decrypt(profile.encrypted_password)
    }

    // ── Shared ─────────────────────────────────────────────────────────────────

    function removeProfile(id: string): void {
        profiles.value = profiles.value.filter(p => p.id !== id)
    }

    function saveCertFingerprint(id: string, fingerprint: string): void {
        const idx = profiles.value.findIndex(p => p.id === id)
        if (idx !== -1) profiles.value[idx] = { ...profiles.value[idx], servercert: fingerprint }
    }

    function replaceState(newProfiles: VpnProfile[]): void {
        profiles.value = newProfiles
    }

    return {
        profiles,
        // WireGuard
        addWireGuardProfile,
        updateWireGuardProfile,
        getDecryptedConfig,
        // OpenConnect
        addOpenConnectProfile,
        updateOpenConnectProfile,
        getDecryptedPassword,
        // Shared
        removeProfile,
        saveCertFingerprint,
        replaceState,
    }
}, {
    persist: {
        storage: TauriStoreAdapter,
    },
})
