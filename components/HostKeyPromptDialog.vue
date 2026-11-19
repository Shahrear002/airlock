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
    prompt_id: number
    session_id: string
    kind: 'unknown' | 'new_key_type' | 'changed'
    host: string
    port: number
    key_type: string
    fingerprint: string
    known: string[]
}

interface HostKeyExpired {
    prompt_id: number
}

const settingsStore = useSettingsStore()

const isOpen = ref(false)
const currentPrompt = ref<HostKeyPrompt | null>(null)
const promptQueue = ref<HostKeyPrompt[]>([])
const copied = ref(false)
const confirmReplace = ref(false)
const isResponding = ref(false)

let unlistenPrompt: UnlistenFn | null = null
let unlistenExpired: UnlistenFn | null = null

onMounted(async () => {
    unlistenPrompt = await listen<HostKeyPrompt>('ssh-host-key-prompt', (event) => {
        if (!isOpen.value) {
            currentPrompt.value = event.payload
            confirmReplace.value = false
            copied.value = false
            isOpen.value = true
        } else {
            promptQueue.value.push(event.payload)
        }
    })

    unlistenExpired = await listen<HostKeyExpired>('ssh-host-key-expired', (event) => {
        const expiredId = event.payload.prompt_id
        if (currentPrompt.value && currentPrompt.value.prompt_id === expiredId) {
            nextPromptOrClose()
        } else {
            promptQueue.value = promptQueue.value.filter(p => p.prompt_id !== expiredId)
        }
    })
})

onBeforeUnmount(() => {
    if (unlistenPrompt) unlistenPrompt()
    if (unlistenExpired) unlistenExpired()
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

const handleDecision = async (decision: 'trust' | 'reject') => {
    if (!currentPrompt.value || isResponding.value) return
    isResponding.value = true

    try {
        await invoke('respond_host_key', {
            promptId: currentPrompt.value.prompt_id,
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
            currentPrompt?.kind === 'changed' ? '!border-red-500/50' : (currentPrompt?.kind === 'new_key_type' ? '!border-amber-500/50' : '')
        ]"
    >
      <DialogHeader>
        <div class="flex items-center gap-3">
          <div 
              class="w-10 h-10 rounded-xl flex items-center justify-center shrink-0 border"
              :class="[
                  currentPrompt?.kind === 'changed' ? 'bg-red-500/10 border-red-500/30 text-red-400' :
                  (currentPrompt?.kind === 'new_key_type' ? 'bg-amber-500/10 border-amber-500/30 text-amber-400' : 'bg-[#2D8CFF]/10 border-[#2D8CFF]/30 text-[#2D8CFF]')
              ]"
          >
            <AlertTriangle v-if="currentPrompt?.kind === 'changed' || currentPrompt?.kind === 'new_key_type'" class="w-5 h-5 animate-pulse" />
            <ShieldCheck v-else class="w-5 h-5" />
          </div>
          <div class="flex-1">
            <DialogTitle 
                class="text-base font-semibold tracking-tight" 
                :class="[
                    currentPrompt?.kind === 'changed' ? 'text-red-400' : 
                    (currentPrompt?.kind === 'new_key_type' ? 'text-amber-400' : 
                    (settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]' : ''))
                ]"
            >
              <template v-if="currentPrompt?.kind === 'changed'">WARNING: Host Key Mismatch!</template>
              <template v-else-if="currentPrompt?.kind === 'new_key_type'">Warning: New Key Type for Known Host</template>
              <template v-else>Unknown SSH Host Key</template>
            </DialogTitle>
            <DialogDescription class="text-xs mt-0.5" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/60' : 'text-muted-foreground'">
              <template v-if="currentPrompt?.kind === 'changed'">Possible Man-in-the-Middle Attack Detected</template>
              <template v-else-if="currentPrompt?.kind === 'new_key_type'">Additional Key Type Detected</template>
              <template v-else>Trust on First Use (TOFU) Verification</template>
            </DialogDescription>
          </div>
        </div>
      </DialogHeader>

      <div v-if="currentPrompt" class="space-y-4 py-2 text-sm">
        <!-- Changed Key (Mismatch) Critical Banner -->
        <div 
            v-if="currentPrompt.kind === 'changed'" 
            class="p-3.5 rounded-lg border border-red-500/30 bg-red-500/10 text-xs leading-relaxed text-red-200"
        >
          <span class="font-bold text-red-300">POTENTIAL SECURITY BREACH:</span>
          The host key presented by the remote server does not match the key previously stored in your known hosts for this algorithm. 
          Someone could be intercepting your connection, or the server's administrator may have regenerated the key.
        </div>

        <!-- New Key Type Warning Banner -->
        <div 
            v-else-if="currentPrompt.kind === 'new_key_type'" 
            class="p-3.5 rounded-lg border border-amber-500/30 bg-amber-500/10 text-xs leading-relaxed text-amber-200"
        >
          <span class="font-bold text-amber-300">ADDITIONAL KEY TYPE:</span>
          This host is already known under a different algorithm, but is now presenting an additional key type (<code class="font-mono text-amber-100">{{ currentPrompt.key_type }}</code>).
          Verify the presented fingerprint before trusting it.
        </div>

        <!-- Neutral TOFU Description -->
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
            <span class="font-mono font-semibold" :class="settingsStore.appTheme === 'glass' ? 'text-[#E9EDF1]' : 'text-foreground'">{{ currentPrompt.host }}:{{ currentPrompt.port }}</span>
          </div>

          <div class="flex items-center justify-between text-[11px] text-muted-foreground font-sans">
            <span class="flex items-center gap-1.5"><KeyRound class="w-3.5 h-3.5" /> Key Algorithm</span>
            <span class="font-mono" :class="settingsStore.appTheme === 'glass' ? 'text-[#E9EDF1]' : 'text-foreground'">{{ currentPrompt.key_type }}</span>
          </div>

          <!-- Existing keys on Changed or NewKeyType -->
          <div v-if="currentPrompt.known && currentPrompt.known.length > 0" class="space-y-1.5 pt-1 border-t border-border/50">
            <div class="text-[11px] font-sans text-amber-400 font-medium">Previously Trusted Key(s):</div>
            <div 
                v-for="(entry, idx) in currentPrompt.known" 
                :key="idx" 
                class="p-2 rounded bg-zinc-900 text-zinc-100 border border-zinc-800 font-mono text-[11px] break-all select-all"
            >
              {{ entry }}
            </div>
          </div>

          <!-- Current Presented Fingerprint -->
          <div class="space-y-1 pt-1 border-t border-border/50">
            <div class="flex items-center justify-between text-[11px] font-sans text-muted-foreground">
              <span :class="currentPrompt.kind === 'changed' ? 'text-red-400 font-semibold' : (currentPrompt.kind === 'new_key_type' ? 'text-amber-400 font-semibold' : '')">
                {{ currentPrompt.kind === 'changed' ? 'New Presented Fingerprint:' : (currentPrompt.kind === 'new_key_type' ? 'New Presented Fingerprint:' : 'Server Fingerprint (SHA-256):') }}
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
                class="p-2 rounded bg-zinc-900 text-zinc-100 border border-zinc-800 font-mono text-[11px] break-all select-all"
                :class="currentPrompt.kind === 'changed' ? '!border-red-500/40' : (currentPrompt.kind === 'new_key_type' ? '!border-amber-500/40' : '')"
            >
              {{ currentPrompt.fingerprint }}
            </div>
          </div>
        </div>

        <!-- Mismatch confirmation checkbox (changed kind only) -->
        <div v-if="currentPrompt.kind === 'changed'" class="flex items-start gap-2 pt-1">
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
          {{ currentPrompt?.kind === 'changed' ? 'Abort Connection (Recommended)' : 'Cancel' }}
        </Button>

        <!-- Acceptance / Replace -->
        <div class="flex items-center gap-2">
          <Button 
              v-if="currentPrompt?.kind === 'changed'" 
              variant="destructive"
              @click="handleDecision('trust')" 
              :disabled="!confirmReplace || isResponding"
              class="text-xs bg-red-600 hover:bg-red-700 text-white"
          >
            Replace Key & Connect
          </Button>

          <Button 
              v-else-if="currentPrompt?.kind === 'new_key_type'"
              @click="handleDecision('trust')" 
              :disabled="isResponding"
              class="text-xs bg-amber-600 hover:bg-amber-700 text-white"
          >
            Trust New Key Type
          </Button>

          <Button 
              v-else 
              @click="handleDecision('trust')" 
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
