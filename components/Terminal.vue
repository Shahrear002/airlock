<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useSessionsStore } from '~/stores/sessions'
import { useSettingsStore } from '~/stores/settings'
import { useTabsStore } from '~/stores/tabs'
import { SplitSquareHorizontal, SplitSquareVertical, X, Copy, Clipboard } from 'lucide-vue-next'

import { onClickOutside } from '@vueuse/core'

const props = defineProps<{
  sessionId: string
}>()

const sessionsStore = useSessionsStore()
const settingsStore = useSettingsStore()
const tabsStore = useTabsStore()

const terminalContainer = ref<HTMLElement | null>(null)
const contextMenuRef = ref<HTMLElement | null>(null)
let term: Terminal | null = null
let fitAddon: FitAddon | null = null
let unlistenOutput: UnlistenFn | null = null
let unlistenError: UnlistenFn | null = null
let unlistenExit: UnlistenFn | null = null
let resizeObserver: ResizeObserver | null = null

const showContextMenu = ref(false)
const contextMenuX = ref(0)
const contextMenuY = ref(0)
const hasSelection = ref(false)


onClickOutside(contextMenuRef, () => {
    showContextMenu.value = false
})

// Ensure terminal fits when tab becomes active
watch(() => tabsStore.activeTabId, async () => {
    await nextTick()
    // Small delay to allow layout to settle
    setTimeout(() => {
        if (terminalContainer.value && terminalContainer.value.clientWidth > 0) {
            fitAndResize()
        }
    }, 50)
})

onMounted(async () => {
// ... existing onMounted code ...
  term = new Terminal({
    cursorBlink: true,
    fontFamily: 'Menlo, Monaco, "Courier New", monospace',
    fontSize: 14,
    theme: settingsStore.currentTheme,
    allowProposedApi: true
  })
  
  // Update theme when it changes
  watch(() => settingsStore.currentTheme, (newTheme) => {
      if (term) {
          term.options.theme = newTheme
      }
  })
  
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  
  if (terminalContainer.value) {
    term.open(terminalContainer.value)
    // fitAddon.fit() // Handled by initial resize
  }

  term.onSelectionChange(() => {
     hasSelection.value = term!.hasSelection()
  })


  // Handle Input (Frontend -> Backend)
  term.onData((data) => {
    invoke('send_ssh_input', { id: props.sessionId, data })
      .catch(err => console.error('Failed to send input', err))
  })

  // Handle Output (Backend -> Frontend)
  unlistenOutput = await listen<Uint8Array>(`ssh-output-${props.sessionId}`, (event) => {
    // event.payload is Uint8Array (or array of numbers). xterm write accepts string or Uint8Array
    // We might need to convert if payload comes as number array from Tauri
    const data = event.payload
    if (term) {
        // Tauri v2 emits byte array as number[] or Uint8Array depending on serialization.
        // Usually creating a Uint8Array works for xterm
        // term.write(new Uint8Array(data))
        // If data is string:
         term.write(typeof data === 'string' ? data : new Uint8Array(data))
    }
  })
  
  unlistenError = await listen<string>(`ssh-error-${props.sessionId}`, (event) => {
      term?.write(`\r\n\x1b[31mError: ${event.payload}\x1b[0m\r\n`)
  })

  // Handle Exit (Backend -> Frontend)
  unlistenExit = await listen<any>(`ssh-exit-${props.sessionId}`, (event) => {
      // Small delay to let final output render?
      setTimeout(() => {
          // Remove session and pane
          sessionsStore.removeSession(props.sessionId)
          tabsStore.removePane(props.sessionId)
      }, 500)
  })
  
  // Use ResizeObserver to handle container resize (window resize or layout changes)
  const fitAndResize = async () => {
    if (!fitAddon || !term || !terminalContainer.value) return
    
    // Skip if hidden
    if (terminalContainer.value.clientWidth === 0 || terminalContainer.value.clientHeight === 0) return

    fitAddon.fit()
    const { rows, cols } = term
    
    try {
        await invoke('resize_pty', { id: props.sessionId, rows, cols })
        // console.log(`Resized PTY to ${cols}x${rows}`)
    } catch (err) {
        // console.warn('Failed to resize PTY:', err)
        // Swallow error if session not ready, but usually we want to know
    }
  }

  const performInitialResize = async (retries = 10, delay = 500) => {
      for (let i = 0; i < retries; i++) {
          try {
              if (terminalContainer.value && terminalContainer.value.clientWidth > 0) {
                 await fitAndResize()
                 // Only return if we actually did something? 
                 // fitAndResize doesn't return value but we can assume success if no error from invoke (if session ready)
                 // But simply running fit() is enough for frontend.
                 console.log('Initial PTY resize successful')
                 return
              }
          } catch (e) {
             // Ignore
          }
           if (i < retries - 1) {
              await new Promise(resolve => setTimeout(resolve, delay))
           }
      }
      console.error('Failed to perform initial PTY resize after multiple attempts')
  }

  resizeObserver = new ResizeObserver(() => {
    // Debounce slightly?
    requestAnimationFrame(() => {
        fitAndResize().catch(e => {
            // Ignore 'Session not found' errors which happen if resize triggers before connection is ready
            if (typeof e === 'string' && e.includes('Session not found')) return
            if (e instanceof Error && e.message.includes('Session not found')) return
            console.error("Resize failed", e)
        })
    })
  })

  if (terminalContainer.value) {
    resizeObserver.observe(terminalContainer.value)
  }

  // Also fit immediately (though observer will likely fire initially), with retry
  setTimeout(() => {
      performInitialResize()
  }, 100)
})

onBeforeUnmount(() => {
  unlistenOutput?.()
  unlistenError?.()
  unlistenExit?.()
  term?.dispose()
  resizeObserver?.disconnect()
})

const activatePane = () => {
    tabsStore.setActivePane(props.sessionId)
}

const onRightClick = (e: MouseEvent) => {
    e.preventDefault()
    showContextMenu.value = true
    contextMenuX.value = e.clientX
    contextMenuY.value = e.clientY
    activatePane()
}

const closeContextMenu = () => {
    showContextMenu.value = false
}

const handleCopy = async () => {
    closeContextMenu()
    const selection = term?.getSelection()
    if (selection) {
        await navigator.clipboard.writeText(selection)
    }
    term?.focus()
}

const handlePaste = async () => {
    closeContextMenu()
    try {
        const text = await navigator.clipboard.readText()
        if (text) {
             invoke('send_ssh_input', { id: props.sessionId, data: text })
                .catch(err => console.error('Failed to paste input', err))
        }
    } catch (err) {
        console.error('Failed to read clipboard', err)
    } finally {
        term?.focus()
    }
}


const split = async (direction: 'horizontal' | 'vertical') => {
    closeContextMenu()
    // 1. Get current session info to replicate credentials (ideal) or just split with new empty/prompt?
    // For now, let's assume we want to "duplicate" the session or start a fresh one?
    // Logic: Create new session -> Add to sessionsStore -> Split in tabsStore
    const currentSession = sessionsStore.sessions.find(s => s.id === props.sessionId)
    if (!currentSession) return

    // Note: We don't have the password anymore (security). 
    // We can either:
    // a) Prompt user again (safest)
    // b) Store password in memory (risky, but convenient for extensive splitting)
    // c) Just open a new session that will prompt (if we had an interactive prompt logic).
    //
    // Since our backend `connect_ssh` takes password, and we don't store it, we might fail to auto-connect.
    // However, the `HostSidebar` logic passes data from the connect form.
    // 
    // CHANGE: For this MVP, let's just create a placeholder session or attempt to reconnect if possible?
    // Realistically, to split *connected* session, we need credentials.
    // Let's assume for now we just split and user has to "connect" again? 
    // Or better: The user initiates split, but we need to know *what* to connect to.
    
    // Simplification: Reuse the same host details but we might lack password.
    // If key-based, it works fine.
    
    // Let's try to find if we can re-trigger connection flow?
    // Maybe we just open the split and show "Disconnected"?
    
    // Let's create a new Session ID first.
    const newSessionId = `session-${Date.now()}`
    
    // We need to add it to sessionsStore
    sessionsStore.addSession({
        id: newSessionId,
        hostLabel: currentSession.hostLabel,
        status: 'disconnected' // Start disconnected?
    })
    
    // Split the pane
    tabsStore.splitPane(props.sessionId, direction, newSessionId)
    
    // Note: It will show empty terminal.
    // TODO: Improve this flow to perhaps prompt for password if needed, or re-use credentials if cached.
}
</script>

<template>
  <div 
    class="h-full w-full relative group" 
    @click="activatePane" 
    @contextmenu="onRightClick"
  >
      <!-- Terminal Container -->
      <div ref="terminalContainer" class="h-full w-full bg-[#282a36] overflow-hidden" />
      
      <!-- Disconnected Overlay -->
      <div 
        v-if="tabsStore.activePaneId === sessionId && sessionsStore.sessions.find(s => s.id === sessionId)?.status === 'disconnected'"
        class="absolute inset-0 flex items-center justify-center bg-black/50 backdrop-blur-sm z-10"
      >
          <div class="text-center p-4 bg-popover/80 border border-border rounded-lg shadow-lg">
               <p class="text-foreground font-medium mb-2">Disconnected</p>
               <p class="text-sm text-muted-foreground">Select a host from the sidebar to connect</p>
          </div>
      </div>
      
      <!-- Context Menu -->
      <div 
        v-if="showContextMenu"
        ref="contextMenuRef"
        class="fixed z-50 bg-popover text-popover-foreground border border-border shadow-md rounded-md p-1 min-w-[150px] flex flex-col gap-0.5"
        :style="{ top: `${contextMenuY}px`, left: `${contextMenuX}px` }"
      >
          <button @click="handleCopy" :disabled="!hasSelection" class="flex items-center gap-2 px-2 py-1.5 hover:bg-muted rounded-sm text-sm text-left disabled:opacity-50 disabled:cursor-not-allowed">
              <Copy class="w-4 h-4" /> Copy
          </button>
          <button @click="handlePaste" class="flex items-center gap-2 px-2 py-1.5 hover:bg-muted rounded-sm text-sm text-left">
              <Clipboard class="w-4 h-4" /> Paste
          </button>
          <div class="h-px bg-border my-0.5"></div>

          <button @click="split('horizontal')" class="flex items-center gap-2 px-2 py-1.5 hover:bg-muted rounded-sm text-sm text-left">
              <SplitSquareVertical class="w-4 h-4" /> Split Horizontal
          </button>
          <button @click="split('vertical')" class="flex items-center gap-2 px-2 py-1.5 hover:bg-muted rounded-sm text-sm text-left">
               <SplitSquareHorizontal class="w-4 h-4" /> Split Vertical
          </button>
           <button @click="sessionsStore.removeSession(sessionId); tabsStore.removePane(sessionId)" class="flex items-center gap-2 px-2 py-1.5 hover:bg-destructive/10 hover:text-destructive rounded-sm text-sm text-left">
               <X class="w-4 h-4" /> Close Pane
          </button>
      </div>
  </div>
</template>


