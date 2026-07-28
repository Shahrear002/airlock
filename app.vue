<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useSessionsStore } from '~/stores/sessions'
import { useTabsStore } from '~/stores/tabs'
import { useVpnStore } from '~/stores/vpn'
import { useVpnConfigsStore } from '~/stores/vpn_configs'
import { invoke } from '@tauri-apps/api/core'
import HostSidebar from './components/HostSidebar.vue'
import SplitPaneLayout from './components/SplitPaneLayout.vue'
import { X, PanelLeft, PanelLeftClose } from 'lucide-vue-next'

const sessionsStore = useSessionsStore()
const tabsStore = useTabsStore()
const vpnStore = useVpnStore()
const vpnConfigsStore = useVpnConfigsStore()
const isSidebarOpen = ref(true)

// ── Resizable sidebar ─────────────────────────────────────────────────────────
const sidebarWidth = ref(260)
const isResizing = ref(false)

function startResize(e: MouseEvent) {
    isResizing.value = true
    const startX = e.clientX
    const startWidth = sidebarWidth.value

    const onMove = (ev: MouseEvent) => {
        const delta = ev.clientX - startX
        sidebarWidth.value = Math.max(200, Math.min(500, startWidth + delta))
    }
    const onUp = () => {
        isResizing.value = false
        window.removeEventListener('mousemove', onMove)
        window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
}

const handleConnect = async (connectionDetails: any) => {
    let id = `session-${Date.now()}`
    const label = `${connectionDetails.username}@${connectionDetails.host}`
    
    // Check if we can reuse the active pane
    const activePaneId = tabsStore.activePaneId
    const activeSession = activePaneId ? sessionsStore.sessions.find(s => s.id === activePaneId) : null
    
    if (activeSession && activeSession.status === 'disconnected') {
        // Reuse existing session/pane
        id = activePaneId! // Use the existing ID
        // Update session details
        activeSession.hostLabel = label
        activeSession.status = 'connected'
        // No need to create a new tab
    } else {
        // Create new session
        sessionsStore.addSession({
            id,
            hostLabel: label,
            status: 'connected'
        })
        // Create a new tab for this session
        tabsStore.createTab(id, label)
    }

    try {
        await invoke('connect_ssh', {
            id,
            host: connectionDetails.host,
            port: Number(connectionDetails.port),
            user: connectionDetails.username,
            password: connectionDetails.password || undefined,
            cols: 80,
            rows: 24
        })
    } catch (e) {
        console.error("Connect failed", e)
        // If connection fails immediately, remove the session/tab?
        // For now, let it stay open so user sees error output
    }
}
onMounted(async () => {
    await vpnStore.init()
})
</script>

<template>
  <div class="flex h-screen bg-background text-foreground font-sans overflow-hidden">
    <!-- Sidebar -->
    <div
      v-show="isSidebarOpen"
      class="flex-shrink-0 h-full relative flex"
      :style="{ width: sidebarWidth + 'px' }"
    >
        <HostSidebar @connect="handleConnect" />
        <!-- Resize handle -->
        <div
            class="absolute right-0 top-0 h-full w-1 cursor-col-resize z-50 group flex items-center justify-center"
            :class="{ 'bg-primary/40': isResizing }"
            @mousedown.prevent="startResize"
        >
            <div class="w-0.5 h-12 rounded-full transition-colors"
                 :class="isResizing ? 'bg-primary' : 'bg-border group-hover:bg-primary/60'" />
        </div>
    </div>

    <!-- Main Content -->
    <div class="flex-1 flex flex-col min-w-0">
        <!-- Tabs Header -->
        <div class="flex items-center border-b border-border bg-card h-10 flex-shrink-0">
             <!-- Sidebar Toggle -->
            <button 
                @click="isSidebarOpen = !isSidebarOpen"
                class="h-full px-3 hover:bg-muted border-r border-border flex items-center justify-center transition-colors"
                :title="isSidebarOpen ? 'Close Sidebar' : 'Open Sidebar'"
            >
                <PanelLeftClose v-if="isSidebarOpen" class="w-4 h-4 text-muted-foreground" />
                <PanelLeft v-else class="w-4 h-4 text-muted-foreground" />
            </button>

             <!-- Tabs Container -->
            <div class="flex-1 flex items-center overflow-x-auto no-scrollbar scroll-smooth">
                <div 
                    v-for="tab in tabsStore.tabs" 
                    :key="tab.id"
                    @click="tabsStore.setActiveTab(tab.id)"
                    :class="['group flex-shrink-0 flex items-center gap-2 px-4 h-full text-sm cursor-pointer border-r border-border hover:bg-muted select-none whitespace-nowrap', tabsStore.activeTabId === tab.id ? 'bg-background font-medium' : 'text-muted-foreground bg-muted/50']"
                >
                    <span>{{ tab.label }}</span>
                    <button 
                        @click.stop="tabsStore.closeTab(tab.id)"
                        class="opacity-0 group-hover:opacity-100 p-0.5 rounded-sm hover:bg-zinc-700/50 transition-all"
                    >
                        <X class="w-3 h-3" />
                    </button>
                </div>
                <div v-if="tabsStore.tabs.length === 0" class="px-4 text-sm text-muted-foreground italic whitespace-nowrap">
                    No active tabs
                </div>
            </div>
        </div>

        <!-- Terminal Area -->
        <div class="flex-1 bg-zinc-950 relative overflow-hidden">
            <template v-if="tabsStore.activeTabId">
                <template v-for="tab in tabsStore.tabs" :key="tab.id">
                     <div v-show="tabsStore.activeTabId === tab.id" class="h-full w-full">
                        <SplitPaneLayout :node="tab.root" />
                     </div>
                </template>
            </template>
            <div v-else class="flex items-center justify-center h-full text-muted-foreground">
                <div class="text-center">
                    <p class="text-lg font-medium text-foreground">Welcome to Airlock</p>
                    <p>Select a host from the sidebar to connect.</p>
                </div>
            </div>
        </div>
    </div>
  </div>
</template>


<style scoped>
/* Hide scrollbar for Chrome, Safari and Opera */
.no-scrollbar::-webkit-scrollbar {
  display: none;
}

/* Hide scrollbar for IE, Edge and Firefox */
.no-scrollbar {
  -ms-overflow-style: none;  /* IE and Edge */
  scrollbar-width: none;  /* Firefox */
}
</style>
