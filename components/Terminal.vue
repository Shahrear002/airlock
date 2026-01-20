<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useSessionsStore } from '~/stores/sessions'

const props = defineProps<{
  sessionId: string
}>()

const sessionsStore = useSessionsStore()

const terminalContainer = ref<HTMLElement | null>(null)
let term: Terminal | null = null
let fitAddon: FitAddon | null = null
let unlistenOutput: UnlistenFn | null = null
let unlistenError: UnlistenFn | null = null
let unlistenExit: UnlistenFn | null = null

onMounted(async () => {
  term = new Terminal({
    cursorBlink: true,
    fontFamily: 'Menlo, Monaco, "Courier New", monospace',
    fontSize: 14,
    theme: {
      background: '#282a36',
      foreground: '#f8f8f2',
      cursor: '#f8f8f2',
      cursorAccent: '#282a36',
      selectionBackground: '#44475a',
      
      black: '#21222c',
      red: '#ff5555',
      green: '#50fa7b',
      yellow: '#f1fa8c',
      blue: '#bd93f9',
      magenta: '#ff79c6',
      cyan: '#8be9fd',
      white: '#f8f8f2',
      
      brightBlack: '#6272a4',
      brightRed: '#ff6e6e',
      brightGreen: '#69ff94',
      brightYellow: '#ffffa5',
      brightBlue: '#d6acff',
      brightMagenta: '#ff92df',
      brightCyan: '#a4ffff',
      brightWhite: '#ffffff',
    }
  })
  
  fitAddon = new FitAddon()
  term.loadAddon(fitAddon)
  
  if (terminalContainer.value) {
    term.open(terminalContainer.value)
    fitAddon.fit()
  }

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
          sessionsStore.removeSession(props.sessionId)
      }, 500)
  })
  
  // Use ResizeObserver to handle container resize (window resize or layout changes)
  const fitAndResize = async () => {
    if (!fitAddon || !term) return
    fitAddon.fit()
    const { rows, cols } = term
    try {
        await invoke('resize_pty', { id: props.sessionId, rows, cols })
    } catch (err) {
        console.warn('Failed to resize PTY, session might not be ready yet:', err)
        throw err
    }
  }

  const performInitialResize = async (retries = 10, delay = 500) => {
      for (let i = 0; i < retries; i++) {
          try {
              await fitAndResize()
              console.log('Initial PTY resize successful')
              return
          } catch (e) {
              if (i < retries - 1) {
                  await new Promise(resolve => setTimeout(resolve, delay))
              }
          }
      }
      console.error('Failed to perform initial PTY resize after multiple attempts')
  }

  const resizeObserver = new ResizeObserver(() => {
    fitAndResize().catch(e => console.error("Resize failed", e))
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
})
</script>

<template>
  <div ref="terminalContainer" class="h-full w-full bg-[#282a36] overflow-hidden" />
</template>
