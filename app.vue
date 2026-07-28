<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useSessionsStore } from '~/stores/sessions'
import { useTabsStore } from '~/stores/tabs'
import { useVpnStore } from '~/stores/vpn'
import { useVpnConfigsStore } from '~/stores/vpn_configs'
import { useSettingsStore } from '~/stores/settings'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import HostSidebar from './components/HostSidebar.vue'
import SplitPaneLayout from './components/SplitPaneLayout.vue'
import { X, PanelLeft, PanelLeftClose, Minus, Square, X as CloseIcon } from 'lucide-vue-next'

const sessionsStore = useSessionsStore()
const tabsStore = useTabsStore()
const vpnStore = useVpnStore()
const vpnConfigsStore = useVpnConfigsStore()
const settingsStore = useSettingsStore()

const appWindow = getCurrentWindow()

const minimizeWindow = () => appWindow.minimize()
const maximizeWindow = () => appWindow.toggleMaximize()
const closeWindow = () => appWindow.close()

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
  <div v-if="settingsStore.appTheme === 'glass'" class="flex flex-col h-screen w-screen overflow-hidden text-[#E9EDF1] font-sans relative bg-[#0A0D14]">
    <div class="absolute inset-0 pointer-events-none overflow-hidden">
        <div class="absolute rounded-full blur-[90px] opacity-35 w-[420px] h-[420px] -top-[140px] -left-[80px] bg-[radial-gradient(circle,_#1DE9B6,_transparent_70%)]"></div>
        <div class="absolute rounded-full blur-[90px] opacity-35 w-[380px] h-[380px] -bottom-[120px] -right-[60px] bg-[radial-gradient(circle,_#6C63FF,_transparent_70%)]"></div>
    </div>

    <div class="titlebar relative z-10">
        <div class="tb-brand"><span class="chevron">&gt;_</span> Airlock</div>
        <div class="tb-spacer"></div>
        <div class="win-controls">
            <div class="win-btn" title="Minimize" @click="minimizeWindow"><svg viewBox="0 0 10 10"><rect x="0" y="4.5" width="10" height="1" fill="currentColor"/></svg></div>
            <div class="win-btn" title="Maximize" @click="maximizeWindow"><svg viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1"/></svg></div>
            <div class="win-btn close" title="Close" @click="closeWindow"><svg viewBox="0 0 10 10"><line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" stroke-width="1.1"/><line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" stroke-width="1.1"/></svg></div>
        </div>
    </div>

    <div class="flex-1 flex min-h-0">
        <div v-show="isSidebarOpen" class="w-[232px] flex-shrink-0 flex flex-col bg-white/5 border-r border-white/5 relative z-10" :style="{ width: sidebarWidth + 'px' }">
            <div class="sidebar-head">
                <span class="label">Explorer</span>
                <div class="sidebar-toggle" title="Collapse sidebar" @click="isSidebarOpen = false">
                    <svg viewBox="0 0 12 12" fill="none"><path d="M8 2L4 6L8 10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto">
                <HostSidebar @connect="handleConnect" />
            </div>
            <div class="absolute right-0 top-0 h-full w-1 cursor-col-resize z-50 group flex items-center justify-center" :class="{ 'bg-primary/40': isResizing }" @mousedown.prevent="startResize">
                <div class="w-0.5 h-12 rounded-full transition-colors" :class="isResizing ? 'bg-primary' : 'bg-border group-hover:bg-primary/60'" />
            </div>
        </div>

        <div class="flex-1 flex flex-col min-w-0">
            <div class="tabstrip">
                <div v-if="!isSidebarOpen" class="flex-shrink-0 flex items-center justify-center w-[42px] border-r border-white/5 hover:bg-white/5 cursor-pointer text-[#E9EDF1]/50 hover:text-white transition-colors" @click="isSidebarOpen = true" title="Expand sidebar">
                    <svg viewBox="0 0 12 12" fill="none" class="w-3 h-3"><path d="M4 2L8 6L4 10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
                </div>
                <div class="fade-edge fade-left"></div>
                <div class="tabstrip-scroll">
                    <div 
                        v-for="tab in tabsStore.tabs" 
                        :key="tab.id"
                        @click="tabsStore.setActiveTab(tab.id)"
                        class="conn-tab group"
                        :class="tabsStore.activeTabId === tab.id ? 'active' : ''"
                    >
                        <span v-if="tabsStore.activeTabId === tab.id" class="live-dot"></span>
                        {{ tab.label }}
                        <span @click.stop="tabsStore.closeTab(tab.id)" class="x transition-colors">✕</span>
                    </div>
                </div>
                <div class="fade-edge fade-right"></div>
            </div>

            <div class="flex-1 relative overflow-hidden bg-black/40">
                <template v-if="tabsStore.activeTabId">
                    <template v-for="tab in tabsStore.tabs" :key="tab.id">
                        <div v-show="tabsStore.activeTabId === tab.id" class="h-full w-full">
                            <SplitPaneLayout :node="tab.root" />
                        </div>
                    </template>
                </template>
                <div v-else class="flex items-center justify-center h-full text-white/50 text-[13px] font-mono">
                    Airlock / Disconnected
                </div>
            </div>
        </div>
    </div>
  </div>

  <div v-else class="flex flex-col h-screen w-screen overflow-hidden text-foreground bg-background font-sans relative transition-colors duration-300">
    <!-- Titlebar (Default Theme) -->
    <div class="h-[38px] flex-shrink-0 flex items-center pl-4 bg-[#FAFAFB] border-b border-border" style="-webkit-app-region: drag;">
        <div class="flex items-center gap-2 text-[13px] font-semibold tracking-wide text-zinc-800">
            <span class="text-[#2D8CFF] font-bold">&gt;_</span> Airlock
        </div>
        <div class="flex-1 h-full" style="-webkit-app-region: drag;"></div>
        <div class="flex items-center h-full" style="-webkit-app-region: no-drag;">
            <div class="w-[38px] h-[38px] flex items-center justify-center cursor-pointer text-zinc-500 hover:bg-zinc-200 hover:text-zinc-900 transition-colors" @click="minimizeWindow"><svg viewBox="0 0 10 10" class="w-[10px] h-[10px]"><rect x="0" y="4.5" width="10" height="1" fill="currentColor"/></svg></div>
            <div class="w-[38px] h-[38px] flex items-center justify-center cursor-pointer text-zinc-500 hover:bg-zinc-200 hover:text-zinc-900 transition-colors" @click="maximizeWindow"><svg viewBox="0 0 10 10" class="w-[10px] h-[10px]"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1"/></svg></div>
            <div class="w-[38px] h-[38px] flex items-center justify-center cursor-pointer text-zinc-500 hover:bg-[#E81123] hover:text-white transition-colors" @click="closeWindow"><svg viewBox="0 0 10 10" class="w-[10px] h-[10px]"><line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" stroke-width="1.1"/><line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" stroke-width="1.1"/></svg></div>
        </div>
    </div>

    <!-- Body Row -->
    <div class="flex-1 flex min-h-0">
        <!-- Sidebar -->
        <div v-show="isSidebarOpen" class="flex-shrink-0 flex flex-col bg-card border-r border-border relative z-10" :style="{ width: sidebarWidth + 'px' }">
            <div class="h-[42px] flex-shrink-0 flex items-center justify-between px-3 border-b border-border">
                <span class="text-[10.5px] uppercase tracking-wide text-muted-foreground font-semibold">Explorer</span>
                <div class="w-[26px] h-[26px] rounded-md flex items-center justify-center text-muted-foreground bg-background border border-border cursor-pointer hover:bg-muted hover:text-foreground transition-colors" @click="isSidebarOpen = false" title="Collapse sidebar">
                    <svg viewBox="0 0 12 12" fill="none" class="w-3 h-3"><path d="M8 2L4 6L8 10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
                </div>
            </div>
            <div class="flex-1 overflow-y-auto">
                <HostSidebar @connect="handleConnect" />
            </div>
            <div class="absolute right-0 top-0 h-full w-1 cursor-col-resize z-50 group flex items-center justify-center" :class="{ 'bg-primary/40': isResizing }" @mousedown.prevent="startResize">
                <div class="w-0.5 h-12 rounded-full transition-colors" :class="isResizing ? 'bg-primary' : 'bg-border group-hover:bg-primary/60'" />
            </div>
        </div>

        <!-- Main Content -->
        <div class="flex-1 flex flex-col min-w-0 bg-background">
            <div class="h-[42px] flex-shrink-0 flex items-stretch border-b border-border bg-card overflow-hidden relative">
                <div v-if="!isSidebarOpen" class="flex-shrink-0 flex items-center justify-center w-[42px] border-r border-border hover:bg-muted cursor-pointer text-muted-foreground hover:text-foreground transition-colors" @click="isSidebarOpen = true" title="Expand sidebar">
                    <svg viewBox="0 0 12 12" fill="none" class="w-3 h-3"><path d="M4 2L8 6L4 10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
                </div>
                <!-- Gradient fade edge (left) -->
                <div class="absolute left-[42px] top-0 bottom-0 w-[24px] bg-gradient-to-r from-card to-transparent z-10 pointer-events-none" v-if="!isSidebarOpen"></div>
                <div class="absolute left-0 top-0 bottom-0 w-[24px] bg-gradient-to-r from-card to-transparent z-10 pointer-events-none" v-else></div>
                
                <div class="flex items-center gap-1.5 px-2.5 overflow-x-auto no-scrollbar relative z-0">
                    <div 
                        v-for="tab in tabsStore.tabs" 
                        :key="tab.id"
                        @click="tabsStore.setActiveTab(tab.id)"
                        class="flex items-center gap-2 px-2.5 py-1.5 rounded-lg text-[12.5px] font-medium whitespace-nowrap flex-shrink-0 cursor-pointer transition-all border group mt-1.5 mb-1.5"
                        :class="tabsStore.activeTabId === tab.id ? 'bg-primary/10 border-primary/30 text-foreground' : 'bg-background border-border text-muted-foreground'"
                    >
                        <span v-if="tabsStore.activeTabId === tab.id" class="w-1.5 h-1.5 rounded-full bg-primary flex-shrink-0"></span>
                        <span>{{ tab.label }}</span>
                        <span @click.stop="tabsStore.closeTab(tab.id)" class="text-[13px] ml-0.5 text-muted-foreground hover:text-foreground">✕</span>
                    </div>
                </div>
                
                <!-- Gradient fade edge (right) -->
                <div class="absolute right-0 top-0 bottom-0 w-[24px] bg-gradient-to-l from-card to-transparent z-10 pointer-events-none"></div>
            </div>

            <div class="flex-1 relative overflow-hidden bg-zinc-950">
                <template v-if="tabsStore.activeTabId">
                    <template v-for="tab in tabsStore.tabs" :key="tab.id">
                        <div v-show="tabsStore.activeTabId === tab.id" class="h-full w-full">
                            <SplitPaneLayout :node="tab.root" />
                        </div>
                    </template>
                </template>
                <div v-else class="flex items-center justify-center h-full text-muted-foreground text-[13px] font-mono">
                    Airlock / Disconnected
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
