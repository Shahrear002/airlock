import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export interface Session {
    id: string
    hostLabel: string // e.g., "user@host"
    status: 'connected' | 'disconnected'
}

export const useSessionsStore = defineStore('sessions', () => {
    const sessions = ref<Session[]>([])
    const activeSessionId = ref<string | null>(null)

    function addSession(session: Session) {
        sessions.value.push(session)
        activeSessionId.value = session.id
    }

    async function removeSession(id: string) {
        // 1. Disconnect via Rust
        try {
            await invoke('disconnect_ssh', { id })
        } catch (e) {
            console.error("Failed to disconnect session", id, e)
        }

        // 2. Remove from state
        sessions.value = sessions.value.filter(s => s.id !== id)

        // 3. Update active session
        if (activeSessionId.value === id) {
            activeSessionId.value = sessions.value.length > 0 ? sessions.value[sessions.value.length - 1].id : null
        }
    }

    function setActive(id: string) {
        if (sessions.value.find(s => s.id === id)) {
            activeSessionId.value = id
        }
    }

    return {
        sessions,
        activeSessionId,
        addSession,
        removeSession,
        setActive
    }
})
