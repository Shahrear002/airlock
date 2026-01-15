<script setup lang="ts">
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import Terminal from './components/Terminal.vue'
import HostSidebar from './components/HostSidebar.vue'

const activeSession = ref<string | null>(null)
const sessions = ref<{id: string, name: string}[]>([])

const handleConnect = async (connectionDetails: any) => {
    const id = `session-${Date.now()}`
    
    // Optimistically create tab
    sessions.value.push({ id, name: `${connectionDetails.username}@${connectionDetails.host}` })
    activeSession.value = id

    try {
        await invoke('connect_ssh', {
            id,
            host: connectionDetails.host,
            port: Number(connectionDetails.port),
            user: connectionDetails.username,
            password: connectionDetails.password || undefined 
        })
    } catch (e) {
        console.error("Connect failed", e)
        // Handle error output in terminal (listener handles it)
    }
}
</script>

<template>
  <div class="flex h-screen bg-background text-foreground font-sans">
    <!-- Sidebar -->
    <HostSidebar @connect="handleConnect" />

    <!-- Main Content -->
    <div class="flex-1 flex flex-col overflow-hidden">
        <!-- Tabs Header -->
        <div class="flex items-center border-b border-border bg-card">
            <div 
                v-for="s in sessions" 
                :key="s.id"
                @click="activeSession = s.id"
                :class="['px-4 py-2 text-sm cursor-pointer border-r border-border hover:bg-muted', activeSession === s.id ? 'bg-background font-medium' : 'text-muted-foreground bg-muted/50']"
            >
                {{ s.name }}
            </div>
            <div v-if="sessions.length === 0" class="px-4 py-2 text-sm text-muted-foreground italic">
                No active sessions
            </div>
        </div>

        <!-- Terminal Area -->
        <div class="flex-1 bg-zinc-950 relative">
            <template v-for="s in sessions" :key="s.id">
                <div v-show="activeSession === s.id" class="absolute inset-0 p-2">
                    <Terminal :session-id="s.id" />
                </div>
            </template>
            <div v-if="sessions.length === 0" class="flex items-center justify-center h-full text-muted-foreground">
                Select a host or create a new connection
            </div>
        </div>
    </div>
  </div>
</template>
