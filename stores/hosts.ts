
import { defineStore } from 'pinia'
import { ref } from 'vue'
import { encrypt, decrypt } from '~/utils/security' // Assuming alias works, or relative path

export interface Host {
    id: string
    name: string
    type: 'host' | 'folder'
    parentId: string | null
    host?: string // Optional for folders
    port?: number // Optional for folders
    username?: string // Optional for folders
    encrypted_password?: string
    private_key_path?: string
}

export const useHostsStore = defineStore('hosts', () => {
    const hosts = ref<Host[]>([])

    // Helper to get children of a node
    const getChildren = (parentId: string | null) => {
        return hosts.value.filter(h => h.parentId === parentId)
    }

    // Helper to recursively delete items
    const deleteRecursive = (id: string) => {
        const children = getChildren(id)
        children.forEach(child => deleteRecursive(child.id))
        hosts.value = hosts.value.filter(h => h.id !== id)
    }

    async function addHost(hostData: Omit<Host, 'id' | 'type' | 'encrypted_password'> & { password?: string }) {
        const id = crypto.randomUUID()
        let encrypted_password = ''

        if (hostData.password) {
            encrypted_password = await encrypt(hostData.password)
        }

        hosts.value.push({
            id,
            type: 'host',
            parentId: hostData.parentId || null,
            name: hostData.name,
            host: hostData.host,
            port: hostData.port,
            username: hostData.username,
            encrypted_password,
            private_key_path: hostData.private_key_path
        })
    }

    function addFolder(name: string, parentId: string | null = null) {
        const id = crypto.randomUUID()
        hosts.value.push({
            id,
            type: 'folder',
            parentId,
            name,
            host: '', // Placeholder
            port: 0, // Placeholder
            username: '' // Placeholder
        })
    }

    function removeItem(id: string) {
        deleteRecursive(id)
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

    function updateFolder(id: string, name: string) {
        const index = hosts.value.findIndex(h => h.id === id)
        if (index === -1) return
        if (hosts.value[index].type !== 'folder') return

        hosts.value[index].name = name
    }

    async function getDecryptedPassword(id: string): Promise<string> {
        const host = hosts.value.find(h => h.id === id)
        if (!host || !host.encrypted_password) return ''
        return await decrypt(host.encrypted_password)
    }

    function replaceState(newHosts: Host[]) {
        hosts.value = newHosts
    }

    async function exportState() {
        // Deep copy to avoid mutating state
        const exportData = JSON.parse(JSON.stringify(hosts.value)) as Host[]

        // Decrypt passwords for export
        for (const host of exportData) {
            if (host.type === 'host' && host.encrypted_password) {
                try {
                    const plainPass = await decrypt(host.encrypted_password)
                    // @ts-ignore - Temporary property for export
                    host.password = plainPass
                    // Remove encrypted version
                    delete (host as any).encrypted_password
                } catch (e) {
                    console.error(`Failed to decrypt password for host ${host.name} during export`, e)
                    // Keep encrypted password or clear it? 
                    // Safest to clear it so we don't export useless local-encrypted data
                    delete (host as any).encrypted_password
                }
            }
        }
        return exportData
    }

    async function importState(newHosts: any[]) {
        const processedHosts: Host[] = []

        for (const item of newHosts) {
            // Validate basic structure
            if (!item.id || !item.type) continue

            const host: Host = {
                id: item.id,
                name: item.name,
                type: item.type,
                parentId: item.parentId,
                host: item.host,
                port: item.port,
                username: item.username,
                private_key_path: item.private_key_path,
                // encrypted_password will be set below
            }

            // If incoming data has 'password', encrypt it locally
            if (item.password) {
                host.encrypted_password = await encrypt(item.password)
            } else if (item.encrypted_password) {
                // If it's legacy data from same machine, using it might work.
                host.encrypted_password = item.encrypted_password
            }

            processedHosts.push(host)
        }

        hosts.value = processedHosts
    }

    return {
        hosts,
        addHost,
        addFolder,
        removeItem,
        updateHost,
        updateFolder,
        getDecryptedPassword,
        getChildren,
        replaceState,
        exportState,
        importState
    }
}, {
    persist: {
        storage: localStorage,
    },
})
