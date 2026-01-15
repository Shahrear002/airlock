<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { Terminal } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

const props = defineProps<{
  sessionId: string
}>()

const terminalContainer = ref<HTMLElement | null>(null)
let term: Terminal | null = null
let fitAddon: FitAddon | null = null
let unlistenOutput: UnlistenFn | null = null
let unlistenError: UnlistenFn | null = null

onMounted(async () => {
  term = new Terminal({
    cursorBlink: true,
    fontFamily: 'Menlo, Monaco, "Courier New", monospace',
    fontSize: 14,
    theme: {
      background: '#1e1e20', // Zinc-900 like
      foreground: '#ffffff',
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
  
  window.addEventListener('resize', onResize)
})

const onResize = () => {
  fitAddon?.fit()
}

onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  unlistenOutput?.()
  unlistenError?.()
  term?.dispose()
})
</script>

<template>
  <div ref="terminalContainer" class="h-full w-full bg-zinc-900 overflow-hidden" />
</template>
