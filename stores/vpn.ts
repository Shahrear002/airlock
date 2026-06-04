import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

export type VpnStatusValue = 'disconnected' | 'connecting' | 'connected' | 'error'

export const useVpnStore = defineStore('vpn', () => {
    const status = ref<VpnStatusValue>('disconnected')
    const error = ref<string | null>(null)
    const activeProfileId = ref<string | null>(null)

    /**
     * Connect the WireGuard VPN tunnel with the provided plaintext config.
     * The config should already be decrypted before calling this.
     */
    async function connect(plaintextConfig: string): Promise<void> {
        if (status.value === 'connected' || status.value === 'connecting') return
        status.value = 'connecting'
        error.value = null
        try {
            await invoke('start_vpn_tunnel', { config: plaintextConfig })
            // status will be updated by the vpn-status-changed event listener
        } catch (e: any) {
            status.value = 'error'
            error.value = typeof e === 'string' ? e : (e?.message ?? 'Unknown VPN error')
            activeProfileId.value = null
            throw e
        }
    }

    /**
     * Disconnect the active WireGuard VPN tunnel.
     */
    async function disconnect(): Promise<void> {
        if (status.value === 'disconnected') return
        error.value = null
        try {
            await invoke('stop_vpn_tunnel')
            activeProfileId.value = null
            // status will be updated by the vpn-status-changed event listener
        } catch (e: any) {
            error.value = typeof e === 'string' ? e : (e?.message ?? 'Unknown VPN error')
            throw e
        }
    }

    /**
     * Hydrate status from the Rust backend on app startup and subscribe to events.
     * Call once from app.vue's onMounted.
     */
    async function init(): Promise<void> {
        try {
            const result = await invoke<{ status: string }>('get_vpn_status')
            status.value = result.status === 'connected' ? 'connected' : 'disconnected'
        } catch {
            status.value = 'disconnected'
        }

        // Subscribe to real-time events emitted by the Rust backend
        await listen<{ status: string }>('vpn-status-changed', (event) => {
            const s = event.payload.status
            if (s === 'connected') {
                status.value = 'connected'
                error.value = null
            } else if (s === 'disconnected') {
                status.value = 'disconnected'
            } else if (s === 'error') {
                status.value = 'error'
            }
        })
    }

    return {
        status,
        error,
        activeProfileId,
        connect,
        disconnect,
        init,
    }
})
