import { defineStore } from 'pinia'
import { ref } from 'vue'
import { encrypt, decrypt } from '~/utils/security'
import { TauriStoreAdapter } from '~/utils/store-adapter'

export interface VpnProfile {
    id: string
    name: string
    encrypted_config: string // AES-256-GCM encrypted WireGuard .conf content
}

export const useVpnConfigsStore = defineStore('vpn_configs', () => {
    const profiles = ref<VpnProfile[]>([])

    async function addProfile(name: string, rawConfig: string): Promise<void> {
        const id = crypto.randomUUID()
        const encrypted_config = await encrypt(rawConfig)
        profiles.value.push({ id, name, encrypted_config })
    }

    async function updateProfile(id: string, name: string, rawConfig: string): Promise<void> {
        const idx = profiles.value.findIndex(p => p.id === id)
        if (idx === -1) return
        const encrypted_config = rawConfig
            ? await encrypt(rawConfig)
            : profiles.value[idx].encrypted_config
        profiles.value[idx] = { id, name, encrypted_config }
    }

    function removeProfile(id: string): void {
        profiles.value = profiles.value.filter(p => p.id !== id)
    }

    async function getDecryptedConfig(id: string): Promise<string> {
        const profile = profiles.value.find(p => p.id === id)
        if (!profile || !profile.encrypted_config) return ''
        return await decrypt(profile.encrypted_config)
    }

    function replaceState(newProfiles: VpnProfile[]): void {
        profiles.value = newProfiles
    }

    return {
        profiles,
        addProfile,
        updateProfile,
        removeProfile,
        getDecryptedConfig,
        replaceState,
    }
}, {
    persist: {
        storage: TauriStoreAdapter,
    },
})
