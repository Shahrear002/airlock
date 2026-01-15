
import { defineStore } from 'pinia'
import { ref } from 'vue'
import { encrypt, decrypt } from '~/utils/security' // Assuming alias works, or relative path

export interface Host {
    id: string
    name: string
    host: string
    port: number
    username: string
    encrypted_password?: string
    private_key_path?: string
}

export const useHostsStore = defineStore('hosts', () => {
    const hosts = ref<Host[]>([])

    async function addHost(hostData: Omit<Host, 'id' | 'encrypted_password'> & { password?: string }) {
        const id = crypto.randomUUID()
        let encrypted_password = ''

        if (hostData.password) {
            encrypted_password = await encrypt(hostData.password)
        }

        hosts.value.push({
            id,
            name: hostData.name,
            host: hostData.host,
            port: hostData.port,
            username: hostData.username,
            encrypted_password,
            private_key_path: hostData.private_key_path
        })
    }

    function removeHost(id: string) {
        hosts.value = hosts.value.filter(h => h.id !== id)
    }

    async function updateHost(id: string, updates: Partial<Host> & { password?: string }) {
        const index = hosts.value.findIndex(h => h.id === id)
        if (index === -1) return

        const host = { ...hosts.value[index], ...updates }

        // If a new password is provided, re-encrypt it
        if (updates.password !== undefined) {
            host.encrypted_password = await encrypt(updates.password)
            delete (host as any).password // Don't store plain password
        }

        hosts.value[index] = host
    }

    async function getDecryptedPassword(id: string): Promise<string> {
        const host = hosts.value.find(h => h.id === id)
        if (!host || !host.encrypted_password) return ''
        return await decrypt(host.encrypted_password)
    }

    return {
        hosts,
        addHost,
        removeHost,
        updateHost,
        getDecryptedPassword
    }
}, {
    persist: {
        storage: localStorage,
    },
})
