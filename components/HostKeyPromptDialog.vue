<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import Dialog from '@/components/ui/dialog/Dialog.vue'
import DialogContent from '@/components/ui/dialog/DialogContent.vue'
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue'
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue'
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue'
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue'
import Button from '@/components/ui/button/Button.vue'
import { ShieldCheck, AlertTriangle, Copy, Check, Server, KeyRound } from 'lucide-vue-next'
import { useSettingsStore } from '~/stores/settings'

interface HostKeyPrompt {
    session_id: string
    host: string
    port: number
    key_type: string
    fingerprint: string
    is_mismatch: boolean
    existing_fingerprint?: string | null
}

const settingsStore = useSettingsStore()

const isOpen = ref(false)
const currentPrompt = ref<HostKeyPrompt | null>(null)
const promptQueue = ref<HostKeyPrompt[]>([])
const copied = ref(false)
const confirmReplace = ref(false)
const isResponding = ref(false)

let unlistenPrompt: UnlistenFn | null = null

onMounted(async () => {
    unlistenPrompt = await listen<HostKeyPrompt>('ssh-host-key-prompt', (event) => {
        if (!isOpen.value) {
            currentPrompt.value = event.payload
            confirmReplace.value = false
            copied.value = false
            isOpen.value = true
        } else {
            // Queue subsequent prompts if multiple simultaneous connections occur
            promptQueue.value.push(event.payload)
        }
    })
})

onBeforeUnmount(() => {
    if (unlistenPrompt) {
        unlistenPrompt()
    }
})

const handleCopyFingerprint = async (text: string) => {
    try {
        await navigator.clipboard.writeText(text)
        copied.value = true
        setTimeout(() => {
            copied.value = false
        }, 2000)
    } catch (e) {
        console.error('Failed to copy fingerprint', e)
    }
}

const nextPromptOrClose = () => {
    if (promptQueue.value.length > 0) {
        currentPrompt.value = promptQueue.value.shift() || null
        confirmReplace.value = false
        copied.value = false
        isOpen.value = true
    } else {
        currentPrompt.value = null
        confirmReplace.value = false
        isOpen.value = false
    }
}

const handleDecision = async (decision: 'accept' | 'replace' | 'reject') => {
    if (!currentPrompt.value || isResponding.value) return
    isResponding.value = true

    try {
        await invoke('respond_host_key', {
            sessionId: currentPrompt.value.session_id,
            decision
        })
    } catch (err) {
        console.error('Failed to send host key decision', err)
    } finally {
        isResponding.value = false
        nextPromptOrClose()
    }
}

const handleDialogClose = (open: boolean) => {
    if (!open && currentPrompt.value) {
        // If dismissed without choosing, reject to fail-safe terminate the connection
        handleDecision('reject')
    }
}
</script>

<template>
  <Dialog :open="isOpen" @update:open="handleDialogClose">
    <DialogContent
        class="sm:max-w-[540px] flex flex-col p-6 border shadow-2xl theme-glass dark"
        :class="[
            settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14]/95 !border-white/10 !text-[#E9EDF1] shadow-[0_0_80px_rgba(0,0,0,0.85)]' : 'bg-background border-border text-foreground',
            currentPrompt?.is_mismatch ? '!border-red-500/50' : ''
        ]"
    >
      <DialogHeader>
        <div class="flex items-center gap-3">
          <div 
              class="w-10 h-10 rounded-xl flex items-center justify-center shrink-0 border"
              :class="currentPrompt?.is_mismatch ? 'bg-red-500/10 border-red-500/30 text-red-400' : 'bg-[#2D8CFF]/10 border-[#2D8CFF]/30 text-[#2D8CFF]'"
          >
            <AlertTriangle v-if="currentPrompt?.is_mismatch" class="w-5 h-5 animate-pulse" />
            <ShieldCheck v-else class="w-5 h-5" />
          </div>
          <div class="flex-1">
            <DialogTitle class="text-base font-semibold tracking-tight" :class="currentPrompt?.is_mismatch ? 'text-red-400' : (settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]' : '')">
              {{ currentPrompt?.is_mismatch ? 'WARNING: Host Key Mismatch!' : 'Unknown SSH Host Key' }}
            </DialogTitle>
            <DialogDescription class="text-xs mt-0.5" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/60' : 'text-muted-foreground'">
              {{ currentPrompt?.is_mismatch ? 'Possible Man-in-the-Middle Attack Detected' : 'Trust on First Use (TOFU) Verification' }}
            </DialogDescription>
          </div>
        </div>
      </DialogHeader>

      <div v-if="currentPrompt" class="space-y-4 py-2 text-sm">
        <!-- Mismatch Critical Warning Box -->
        <div 
            v-if="currentPrompt.is_mismatch" 
            class="p-3.5 rounded-lg border border-red-500/30 bg-red-500/10 text-xs leading-relaxed text-red-200"
        >
          <span class="font-bold text-red-300">POTENTIAL SECURITY BREACH:</span>
          The host key presented by the remote server does not match the key previously stored in your known hosts. 
          Someone could be intercepting your connection, or the server's administrator may have regenerated the key.
        </div>

        <!-- Normal TOFU description -->
        <p v-else class="text-xs leading-relaxed" :class="settingsStore.appTheme === 'glass' ? 'text-white/70' : 'text-muted-foreground'">
          The authenticity of host <code class="px-1.5 py-0.5 rounded font-mono text-[#2D8CFF] bg-[#2D8CFF]/10 border border-[#2D8CFF]/20">{{ currentPrompt.host }}:{{ currentPrompt.port }}</code> cannot be established.
          Verify the key fingerprint below before connecting.
        </p>

        <!-- Host Details Card -->
        <div 
            class="rounded-lg border p-3.5 space-y-2.5 font-mono text-xs"
            :class="settingsStore.appTheme === 'glass' ? 'bg-white/[0.03] border-white/10' : 'bg-muted/40 border-border'"
        >
          <div class="flex items-center justify-between text-[11px] text-muted-foreground font-sans">
            <span class="flex items-center gap-1.5"><Server class="w-3.5 h-3.5" /> Target Server</span>
            <span class="font-mono text-foreground font-semibold">{{ currentPrompt.host }}:{{ currentPrompt.port }}</span>
          </div>

          <div class="flex items-center justify-between text-[11px] text-muted-foreground font-sans">
            <span class="flex items-center gap-1.5"><KeyRound class="w-3.5 h-3.5" /> Key Algorithm</span>
            <span class="font-mono text-foreground">{{ currentPrompt.key_type }}</span>
          </div>

          <!-- Existing fingerprint on mismatch -->
          <div v-if="currentPrompt.is_mismatch && currentPrompt.existing_fingerprint" class="space-y-1 pt-1 border-t border-border/50">
            <div class="text-[11px] font-sans text-amber-400 font-medium">Previously Trusted Fingerprint:</div>
            <div class="p-2 rounded bg-black/30 border border-white/5 font-mono text-[11px] text-amber-200/90 break-all select-all">
              {{ currentPrompt.existing_fingerprint }}
            </div>
          </div>

          <!-- Current Fingerprint -->
          <div class="space-y-1 pt-1 border-t border-border/50">
            <div class="flex items-center justify-between text-[11px] font-sans text-muted-foreground">
              <span :class="currentPrompt.is_mismatch ? 'text-red-400 font-semibold' : ''">
                {{ currentPrompt.is_mismatch ? 'New Presented Fingerprint:' : 'Server Fingerprint (SHA-256):' }}
              </span>
              <button 
                  @click="handleCopyFingerprint(currentPrompt.fingerprint)" 
                  class="flex items-center gap-1 text-[10px] px-1.5 py-0.5 rounded hover:bg-white/10 transition-colors"
                  :class="copied ? 'text-emerald-400' : 'text-muted-foreground hover:text-foreground'"
                  title="Copy fingerprint"
              >
                <Check v-if="copied" class="w-3 h-3" />
                <Copy v-else class="w-3 h-3" />
                <span>{{ copied ? 'Copied' : 'Copy' }}</span>
              </button>
            </div>
            <div 
                class="p-2 rounded bg-black/40 border font-mono text-[11px] break-all select-all"
                :class="currentPrompt.is_mismatch ? 'border-red-500/40 text-red-200 bg-red-950/20' : 'border-white/10 text-foreground'"
            >
              {{ currentPrompt.fingerprint }}
            </div>
          </div>
        </div>

        <!-- Mismatch confirmation checkbox -->
        <div v-if="currentPrompt.is_mismatch" class="flex items-start gap-2 pt-1">
          <input 
              type="checkbox" 
              id="confirm-replace-key" 
              v-model="confirmReplace" 
              class="mt-1 rounded border-border text-red-500 focus:ring-red-500 cursor-pointer"
          />
          <label for="confirm-replace-key" class="text-xs text-muted-foreground cursor-pointer select-none">
            I verify that this host key change is expected (e.g. server reinstall) and wish to permanently replace the stored key.
          </label>
        </div>
      </div>

      <DialogFooter class="flex sm:justify-between items-center gap-2 pt-3 border-t border-border/40">
        <!-- Rejection / Abort (Safe Action) -->
        <Button 
            variant="outline" 
            @click="handleDecision('reject')" 
            :disabled="isResponding"
            class="text-xs"
            :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''"
        >
          {{ currentPrompt?.is_mismatch ? 'Abort Connection (Recommended)' : 'Cancel' }}
        </Button>

        <!-- Acceptance / Replace -->
        <div class="flex items-center gap-2">
          <Button 
              v-if="currentPrompt?.is_mismatch" 
              variant="destructive"
              @click="handleDecision('replace')" 
              :disabled="!confirmReplace || isResponding"
              class="text-xs bg-red-600 hover:bg-red-700 text-white"
          >
            Replace Key & Connect
          </Button>

          <Button 
              v-else 
              @click="handleDecision('accept')" 
              :disabled="isResponding"
              class="text-xs"
              :class="settingsStore.appTheme === 'glass' ? 'btn-primary' : 'bg-primary text-primary-foreground'"
          >
            Trust & Connect
          </Button>
        </div>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
