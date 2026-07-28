<script setup lang="ts">
import { ref, computed } from 'vue'
import { useVpnConfigsStore, type OpenConnectProtocolHint } from '~/stores/vpn_configs'
import { useVpnStore } from '~/stores/vpn'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import {
    Shield, ShieldCheck, ShieldOff, ShieldAlert, Loader2,
    Plus, Pencil, Trash2, Wifi, X, KeyRound, Globe, Lock
} from 'lucide-vue-next'
import Dialog from '@/components/ui/dialog/Dialog.vue'
import DialogContent from '@/components/ui/dialog/DialogContent.vue'
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue'
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue'
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue'
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue'
import Button from '@/components/ui/button/Button.vue'
import Input from '@/components/ui/input/Input.vue'
import Label from '@/components/ui/label/Label.vue'
import { useSettingsStore } from '~/stores/settings'

const vpnConfigsStore = useVpnConfigsStore()
const vpnStore = useVpnStore()
const settingsStore = useSettingsStore()

// ── Dialog state ──────────────────────────────────────────────────────────────
const isDialogOpen = ref(false)
const editingId = ref<string | null>(null)
const confirmDeleteId = ref<string | null>(null)
const isSaving = ref(false)
const formError = ref('')

// ── Add / Edit form ───────────────────────────────────────────────────────────
const form = ref({
    name: '',
    protocol: 'wireguard' as 'wireguard' | 'openconnect',
    // WireGuard
    wg_config: '',
    // OpenConnect
    server_url: '',
    port: 443 as number | undefined,
    username: '',
    password: '',
    protocol_hint: 'auto' as OpenConnectProtocolHint,
})

// ── Connection state ──────────────────────────────────────────────────────────
const connectingId = ref<string | null>(null)
const connectionError = ref('')

// ── 2FA dialog ────────────────────────────────────────────────────────────────
const isMfaDialogOpen = ref(false)
const mfaPrompt = ref('')
const mfaToken = ref('')
const mfaSubmitting = ref(false)

// Listen for MFA prompts from backend
listen<{ prompt: string }>('vpn-mfa-required', (event) => {
    mfaPrompt.value = event.payload.prompt || 'Enter your authentication token'
    mfaToken.value = ''
    isMfaDialogOpen.value = true
})

// ── Certificate trust dialog ──────────────────────────────────────────────────
const isCertDialogOpen = ref(false)
const certFingerprint = ref('')
const certServer = ref('')
const certProfileId = ref<string | null>(null)
const certTrusting = ref(false)

listen<{ fingerprint: string; server: string }>('vpn-cert-verify', (event) => {
    certFingerprint.value = event.payload.fingerprint
    certServer.value = event.payload.server
    isCertDialogOpen.value = true
    // Reset VPN status since the process exited
    vpnStore.status = 'disconnected'
})

async function trustCertAndReconnect() {
    if (!certProfileId.value || !certFingerprint.value) return
    certTrusting.value = true
    try {
        // 1. Save fingerprint to profile
        vpnConfigsStore.saveCertFingerprint(certProfileId.value, certFingerprint.value)
        isCertDialogOpen.value = false
        // 2. Reconnect with the saved fingerprint
        await connectProfile(certProfileId.value)
    } finally {
        certTrusting.value = false
    }
}

// ── Computed ──────────────────────────────────────────────────────────────────
const isEditing = computed(() => editingId.value !== null)

const statusDotClass = computed(() => ({
    'bg-green-500': vpnStore.status === 'connected',
    'bg-amber-400 animate-pulse': vpnStore.status === 'connecting',
    'bg-red-500': vpnStore.status === 'error',
    'bg-zinc-600': vpnStore.status === 'disconnected',
}))

const statusColor = computed(() => ({
    'text-green-400': vpnStore.status === 'connected',
    'text-amber-400': vpnStore.status === 'connecting',
    'text-red-400': vpnStore.status === 'error',
    'text-zinc-400': vpnStore.status === 'disconnected',
}))

const statusLabel = computed(() => ({
    connected: 'Connected',
    connecting: 'Connecting...',
    error: 'Connection Error',
    disconnected: 'Disconnected',
}[vpnStore.status]))

const openConnectProtocols: { value: OpenConnectProtocolHint; label: string; description: string }[] = [
    { value: 'auto', label: 'Auto-detect', description: 'Let OpenConnect detect the server protocol' },
    { value: 'anyconnect', label: 'Cisco AnyConnect', description: 'Cisco ASA, Cisco FTD' },
    { value: 'gp', label: 'GlobalProtect', description: 'Palo Alto Networks' },
    { value: 'pulse', label: 'Pulse / Ivanti', description: 'Pulse Secure, Ivanti Connect' },
    { value: 'f5', label: 'F5 BIG-IP', description: 'F5 Access Policy Manager' },
    { value: 'fortinet', label: 'Fortinet', description: 'FortiGate FortiSSL' },
]

// ── Dialog helpers ────────────────────────────────────────────────────────────
function resetForm() {
    editingId.value = null
    formError.value = ''
    form.value = {
        name: '',
        protocol: 'wireguard',
        wg_config: '',
        server_url: '',
        port: 443,
        username: '',
        password: '',
        protocol_hint: 'auto',
    }
}

function openAddDialog() {
    resetForm()
    isDialogOpen.value = true
}

async function openEditDialog(id: string) {
    const profile = vpnConfigsStore.profiles.find(p => p.id === id)
    if (!profile) return
    editingId.value = id
    formError.value = ''

    if (profile.protocol === 'wireguard') {
        const decrypted = await vpnConfigsStore.getDecryptedConfig(id)
        form.value = {
            name: profile.name,
            protocol: 'wireguard',
            wg_config: decrypted,
            server_url: '',
            username: '',
            password: '',
            protocol_hint: 'auto',
        }
    } else {
        form.value = {
            name: profile.name,
            protocol: 'openconnect',
            wg_config: '',
            server_url: profile.server_url ?? '',
            port: profile.port ?? 443,
            username: profile.username ?? '',
            password: '',
            protocol_hint: profile.protocol_hint ?? 'auto',
        }
    }
    isDialogOpen.value = true
}

async function saveProfile() {
    formError.value = ''

    if (!form.value.name.trim()) {
        formError.value = 'Profile name is required.'
        return
    }

    if (form.value.protocol === 'wireguard') {
        if (!form.value.wg_config.trim()) {
            formError.value = 'WireGuard configuration is required.'
            return
        }
        if (!form.value.wg_config.includes('[Interface]') || !form.value.wg_config.includes('[Peer]')) {
            formError.value = 'Invalid WireGuard config — must contain [Interface] and [Peer] sections.'
            return
        }
    } else {
        if (!form.value.server_url.trim()) {
            formError.value = 'Server URL is required.'
            return
        }
        if (!form.value.username.trim()) {
            formError.value = 'Username is required.'
            return
        }
        if (!isEditing.value && !form.value.password.trim()) {
            formError.value = 'Password is required.'
            return
        }
    }

    isSaving.value = true
    try {
        if (form.value.protocol === 'wireguard') {
            if (isEditing.value && editingId.value) {
                await vpnConfigsStore.updateWireGuardProfile(editingId.value, form.value.name.trim(), form.value.wg_config.trim())
            } else {
                await vpnConfigsStore.addWireGuardProfile(form.value.name.trim(), form.value.wg_config.trim())
            }
        } else {
            if (isEditing.value && editingId.value) {
                await vpnConfigsStore.updateOpenConnectProfile(
                    editingId.value,
                    form.value.name.trim(),
                    form.value.server_url.trim(),
                    form.value.port || undefined,
                    form.value.username.trim(),
                    form.value.password,
                    form.value.protocol_hint,
                )
            } else {
                await vpnConfigsStore.addOpenConnectProfile(
                    form.value.name.trim(),
                    form.value.server_url.trim(),
                    form.value.port || undefined,
                    form.value.username.trim(),
                    form.value.password.trim(),
                    form.value.protocol_hint,
                )
            }
        }
        isDialogOpen.value = false
        resetForm()
    } finally {
        isSaving.value = false
    }
}

// ── Connect / Disconnect ──────────────────────────────────────────────────────
async function connectProfile(id: string) {
    const profile = vpnConfigsStore.profiles.find(p => p.id === id)
    if (!profile) return

    // Disconnect any existing tunnel first
    if (vpnStore.status !== 'disconnected') {
        await vpnStore.disconnect().catch(() => {})
    }

    connectingId.value = id
    connectionError.value = ''
    certProfileId.value = id  // track which profile is connecting for cert trust dialog

    try {
        if (profile.protocol === 'wireguard') {
            const config = await vpnConfigsStore.getDecryptedConfig(id)
            if (!config) throw new Error('Failed to decrypt WireGuard configuration')
            vpnStore.activeProfileId = id
            await vpnStore.connect(config)
        } else {
            // OpenConnect
            const password = await vpnConfigsStore.getDecryptedPassword(id)
            vpnStore.activeProfileId = id
            await invoke('start_openconnect_tunnel', {
                server: profile.server_url ?? '',
                port: profile.port ?? null,
                servercert: profile.servercert ?? null,
                username: profile.username ?? '',
                password,
                protocolHint: profile.protocol_hint ?? 'auto',
            })
            // Status will be updated via vpn-status-changed event from backend
        }
    } catch (e: any) {
        connectionError.value = typeof e === 'string' ? e : (e?.message ?? 'Connection failed')
        vpnStore.activeProfileId = null
    } finally {
        connectingId.value = null
    }
}

async function disconnect() {
    connectionError.value = ''
    try {
        await vpnStore.disconnect()
        vpnStore.activeProfileId = null
    } catch (e: any) {
        connectionError.value = typeof e === 'string' ? e : (e?.message ?? 'Disconnect failed')
    }
}

// ── 2FA ───────────────────────────────────────────────────────────────────────
async function submitMfaToken() {
    if (!mfaToken.value.trim()) return
    mfaSubmitting.value = true
    try {
        await invoke('send_mfa_token', { token: mfaToken.value.trim() })
        isMfaDialogOpen.value = false
        mfaToken.value = ''
    } catch (e: any) {
        connectionError.value = typeof e === 'string' ? e : (e?.message ?? 'Failed to submit token')
    } finally {
        mfaSubmitting.value = false
    }
}

// ── Delete ────────────────────────────────────────────────────────────────────
function confirmDelete(id: string) { confirmDeleteId.value = id }
function doDelete() {
    if (!confirmDeleteId.value) return
    if (vpnStore.status !== 'disconnected' && vpnStore.activeProfileId === confirmDeleteId.value) {
        vpnStore.disconnect().catch(() => {})
    }
    vpnConfigsStore.removeProfile(confirmDeleteId.value)
    confirmDeleteId.value = null
}

// ── Helpers ───────────────────────────────────────────────────────────────────
function protocolLabel(hint: OpenConnectProtocolHint | undefined) {
    return openConnectProtocols.find(p => p.value === (hint ?? 'auto'))?.label ?? 'Auto-detect'
}
</script>

<template>
  <div class="flex flex-col h-full">

    <!-- ── Header ─────────────────────────────────────────────────────────── -->
    <div class="flex items-center justify-between px-4 py-3 border-b border-border flex-shrink-0">
        <div class="flex items-center gap-2">
            <Shield class="w-4 h-4 text-primary" />
            <span class="text-sm font-semibold">VPN Profiles</span>
        </div>
        <button
            @click="openAddDialog"
            class="flex items-center gap-1 text-xs px-2.5 py-1.5 rounded-md bg-primary/10 hover:bg-primary/20 text-primary border border-primary/30 transition-colors"
        >
            <Plus class="w-3.5 h-3.5" />
            Add Profile
        </button>
    </div>

    <!-- ── Status Banner ─────────────────────────────────────────────────── -->
    <div
        class="mx-3 mt-3 rounded-lg border px-3 py-2.5 flex-shrink-0 transition-all"
        :class="{
            'border-green-500/40 bg-green-500/5': vpnStore.status === 'connected',
            'border-amber-400/40 bg-amber-400/5': vpnStore.status === 'connecting',
            'border-red-500/40 bg-red-500/5': vpnStore.status === 'error',
            'border-border bg-muted/20': vpnStore.status === 'disconnected',
        }"
    >
        <div class="flex items-center justify-between gap-2">
            <div class="flex items-center gap-2">
                <span class="relative flex h-2.5 w-2.5 flex-shrink-0">
                    <span v-if="vpnStore.status === 'connected'"
                        class="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75" />
                    <span class="relative inline-flex rounded-full h-2.5 w-2.5" :class="statusDotClass" />
                </span>
                <span class="text-xs font-medium" :class="statusColor">{{ statusLabel }}</span>
                <Loader2 v-if="vpnStore.status === 'connecting'" class="w-3 h-3 animate-spin" :class="statusColor" />
            </div>
            <button
                v-if="vpnStore.status !== 'disconnected' && vpnStore.status !== 'connecting'"
                @click="disconnect"
                class="text-xs px-2 py-0.5 rounded border border-red-500/40 text-red-400 hover:bg-red-500/10 transition-colors"
            >
                Disconnect
            </button>
        </div>
        <p v-if="connectionError" class="text-xs text-red-400 mt-1.5 leading-snug break-words">
            {{ connectionError }}
        </p>
    </div>

    <!-- ── Profile List ───────────────────────────────────────────────────── -->
    <div class="flex-1 overflow-y-auto px-3 py-3 space-y-2">

        <!-- Empty state -->
        <div v-if="vpnConfigsStore.profiles.length === 0"
            class="flex flex-col items-center justify-center h-full gap-3 text-center py-8"
        >
            <div class="w-12 h-12 rounded-full bg-muted/50 flex items-center justify-center">
                <ShieldOff class="w-6 h-6 text-muted-foreground" />
            </div>
            <div>
                <p class="text-sm font-medium text-foreground">No VPN profiles</p>
                <p class="text-xs text-muted-foreground mt-0.5">Add a WireGuard or OpenConnect config</p>
            </div>
            <button @click="openAddDialog"
                class="text-xs px-3 py-1.5 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 transition-colors">
                Add Profile
            </button>
        </div>

        <!-- Profile cards -->
        <div
            v-for="profile in vpnConfigsStore.profiles"
            :key="profile.id"
            class="rounded-lg border transition-all overflow-hidden"
            :class="{
                'border-green-500/50 bg-green-500/5': vpnStore.status === 'connected' && vpnStore.activeProfileId === profile.id,
                'border-border bg-muted/20 hover:bg-muted/30': !(vpnStore.status === 'connected' && vpnStore.activeProfileId === profile.id),
            }"
        >
            <!-- Row 1: Icon + Name + Badge + Edit/Delete -->
            <div class="flex items-center gap-2 px-3 pt-2.5 pb-1">
                <ShieldCheck v-if="vpnStore.status === 'connected' && vpnStore.activeProfileId === profile.id"
                    class="w-4 h-4 text-green-400 flex-shrink-0" />
                <Shield v-else class="w-4 h-4 text-muted-foreground flex-shrink-0" />

                <p class="text-sm font-medium truncate flex-1">{{ profile.name }}</p>

                <!-- Protocol badge -->
                <span
                    class="text-[10px] font-bold px-1.5 py-0.5 rounded flex-shrink-0"
                    :class="{
                        'bg-blue-500/20 text-blue-400 border border-blue-500/30': profile.protocol === 'wireguard',
                        'bg-purple-500/20 text-purple-400 border border-purple-500/30': profile.protocol === 'openconnect',
                    }"
                >
                    {{ profile.protocol === 'wireguard' ? 'WG' : 'OC' }}
                </span>

                <button @click="openEditDialog(profile.id)"
                    class="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-muted transition-colors flex-shrink-0">
                    <Pencil class="w-3 h-3" />
                </button>
                <button @click="confirmDelete(profile.id)"
                    class="p-1 rounded text-muted-foreground hover:text-red-400 hover:bg-red-500/10 transition-colors flex-shrink-0">
                    <Trash2 class="w-3 h-3" />
                </button>
            </div>

            <!-- Row 2: Subtitle -->
            <p class="text-xs text-muted-foreground truncate px-3 pb-2">
                <template v-if="profile.protocol === 'wireguard'">WireGuard · Encrypted config</template>
                <template v-else>{{ profile.server_url }}<span v-if="profile.port && profile.port !== 443">:{{ profile.port }}</span> · {{ protocolLabel(profile.protocol_hint) }}</template>
            </p>

            <!-- Row 3: Connect / Disconnect full-width button -->
            <div class="px-2 pb-2.5">
                <!-- Disconnect (active profile) -->
                <button
                    v-if="vpnStore.status === 'connected' && vpnStore.activeProfileId === profile.id"
                    @click="disconnect"
                    class="w-full text-xs py-1.5 rounded-md border border-red-500/40 bg-red-500/5 text-red-400 hover:bg-red-500/10 transition-colors font-medium"
                >
                    ⏻ Disconnect
                </button>
                <!-- Connecting (this profile) -->
                <button
                    v-else-if="connectingId === profile.id"
                    disabled
                    class="w-full text-xs py-1.5 rounded-md border border-border bg-muted/30 text-muted-foreground flex items-center justify-center gap-1.5"
                >
                    <Loader2 class="w-3 h-3 animate-spin" />
                    Connecting...
                </button>
                <!-- Connect -->
                <button
                    v-else
                    @click="connectProfile(profile.id)"
                    :disabled="vpnStore.status === 'connecting'"
                    class="w-full text-xs py-1.5 rounded-md border border-primary/40 bg-primary/5 text-primary hover:bg-primary/10 transition-colors font-medium disabled:opacity-50 disabled:cursor-not-allowed"
                >
                    Connect
                </button>
            </div>
        </div>
    </div>

    <!-- ── Add / Edit Dialog ──────────────────────────────────────────────── -->
    <Dialog v-model:open="isDialogOpen">
        <DialogContent 
            class="w-[95vw] max-w-[540px] max-h-[85vh] flex flex-col resize overflow-hidden min-w-[320px] min-h-[400px]"
            :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-[#E9EDF1] shadow-[0_0_80px_rgba(0,0,0,0.8)]' : ''"
        >
            <DialogHeader class="flex-shrink-0">
                <DialogTitle class="flex items-center gap-2" :class="settingsStore.appTheme === 'glass' ? 'text-[15px] !text-[#E9EDF1]' : ''">
                    <Shield class="w-4 h-4 text-primary" />
                    {{ isEditing ? 'Edit VPN Profile' : '🛡 Add VPN Profile' }}
                </DialogTitle>
                <DialogDescription :class="settingsStore.appTheme === 'glass' ? 'hint' : ''">
                    Passwords and keys are encrypted with AES-256 before being stored.
                </DialogDescription>
            </DialogHeader>

            <div class="flex flex-col gap-4 py-2 px-1 flex-1 overflow-y-auto">

                <!-- Profile name -->
                <div class="flex flex-col gap-1.5">
                    <Label htmlFor="vpn-name" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Profile Name</Label>
                    <Input id="vpn-name" v-model="form.name" placeholder="e.g. Work VPN, Mullvad US, Home Lab" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#1DE9B6]/40' : ''" />
                </div>

                <!-- Protocol selector (only when adding) -->
                <div v-if="!isEditing" class="flex flex-col gap-1.5">
                    <Label :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Protocol</Label>
                    <div class="grid grid-cols-2 gap-2">
                        <button
                            @click="form.protocol = 'wireguard'"
                            class="flex flex-col items-start gap-1 p-3 rounded-lg border transition-all text-left"
                            :class="[
                                form.protocol === 'wireguard' ? (settingsStore.appTheme === 'glass' ? 'border-[#1DE9B6]/50 bg-[#1DE9B6]/10' : 'border-blue-500/60 bg-blue-500/10') : (settingsStore.appTheme === 'glass' ? 'border-white/10 hover:border-white/20 hover:bg-white/5' : 'border-border hover:border-border/80 hover:bg-muted/30')
                            ]"
                        >
                            <div class="flex items-center gap-2">
                                <KeyRound class="w-4 h-4" :class="form.protocol === 'wireguard' ? (settingsStore.appTheme === 'glass' ? 'text-[#1DE9B6]' : 'text-blue-400') : 'text-muted-foreground'" />
                                <span class="text-sm font-semibold" :class="form.protocol === 'wireguard' ? (settingsStore.appTheme === 'glass' ? 'text-[#1DE9B6]' : 'text-blue-400') : (settingsStore.appTheme === 'glass' ? 'text-white/80' : '')">WireGuard</span>
                                <span class="text-[10px] font-bold px-1 py-0.5 rounded border" :class="form.protocol === 'wireguard' ? (settingsStore.appTheme === 'glass' ? 'bg-[#1DE9B6]/20 text-[#1DE9B6] border-[#1DE9B6]/30' : 'bg-blue-500/20 text-blue-400 border-blue-500/30') : 'bg-transparent text-muted-foreground border-border/50'">WG</span>
                            </div>
                            <p class="text-xs" :class="settingsStore.appTheme === 'glass' ? 'text-white/40' : 'text-muted-foreground'">Paste a .conf file — for personal VPNs, Mullvad, ProtonVPN</p>
                        </button>

                        <button
                            @click="form.protocol = 'openconnect'"
                            class="flex flex-col items-start gap-1 p-3 rounded-lg border transition-all text-left"
                            :class="[
                                form.protocol === 'openconnect' ? (settingsStore.appTheme === 'glass' ? 'border-[#2D8CFF]/50 bg-[#2D8CFF]/10' : 'border-purple-500/60 bg-purple-500/10') : (settingsStore.appTheme === 'glass' ? 'border-white/10 hover:border-white/20 hover:bg-white/5' : 'border-border hover:border-border/80 hover:bg-muted/30')
                            ]"
                        >
                            <div class="flex items-center gap-2">
                                <Globe class="w-4 h-4" :class="form.protocol === 'openconnect' ? (settingsStore.appTheme === 'glass' ? 'text-[#2D8CFF]' : 'text-purple-400') : 'text-muted-foreground'" />
                                <span class="text-sm font-semibold" :class="form.protocol === 'openconnect' ? (settingsStore.appTheme === 'glass' ? 'text-[#2D8CFF]' : 'text-purple-400') : (settingsStore.appTheme === 'glass' ? 'text-white/80' : '')">OpenConnect</span>
                                <span class="text-[10px] font-bold px-1 py-0.5 rounded border" :class="form.protocol === 'openconnect' ? (settingsStore.appTheme === 'glass' ? 'bg-[#2D8CFF]/20 text-[#2D8CFF] border-[#2D8CFF]/30' : 'bg-purple-500/20 text-purple-400 border-purple-500/30') : 'bg-transparent text-muted-foreground border-border/50'">OC</span>
                            </div>
                            <p class="text-xs" :class="settingsStore.appTheme === 'glass' ? 'text-white/40' : 'text-muted-foreground'">URL + credentials — for Cisco, GlobalProtect, Fortinet</p>
                        </button>
                    </div>
                </div>

                <!-- ── WireGuard fields ──────────────────────────────────── -->
                <template v-if="form.protocol === 'wireguard'">
                    <div class="flex flex-col gap-1.5">
                        <div class="flex items-center justify-between">
                            <Label htmlFor="wg-config" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">WireGuard Configuration</Label>
                            <span class="text-xs" :class="settingsStore.appTheme === 'glass' ? 'text-white/40' : 'text-muted-foreground'">Paste your .conf file</span>
                        </div>
                        <textarea
                            id="wg-config"
                            v-model="form.wg_config"
                            rows="7"
                            placeholder="[Interface]
PrivateKey = <your-private-key>
Address = 10.8.0.2/24
DNS = 1.1.1.1

[Peer]
PublicKey = <server-public-key>
Endpoint = vpn.example.com:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25"
                            class="w-full resize-y focus:outline-none placeholder:text-muted-foreground/50 leading-relaxed"
                            :class="settingsStore.appTheme === 'glass' ? 'code-area' : 'rounded-md border border-input bg-background px-3 py-2 text-xs font-mono focus:ring-2 focus:ring-ring text-foreground min-h-[140px] max-h-[260px]'"
                        />
                    </div>
                </template>

                <!-- ── OpenConnect fields ───────────────────────────────── -->
                <template v-else>
                    <!-- Server URL + Port -->
                    <div class="flex gap-2">
                        <div class="flex flex-col gap-1.5 flex-1">
                            <Label htmlFor="oc-server" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Server URL</Label>
                            <Input id="oc-server" v-model="form.server_url"
                                placeholder="vpn.company.com" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                        </div>
                        <div class="flex flex-col gap-1.5 w-24">
                            <Label htmlFor="oc-port" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Port</Label>
                            <Input id="oc-port" v-model.number="form.port" type="number"
                                min="1" max="65535" placeholder="443" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                        </div>
                    </div>

                    <!-- Protocol hint -->
                    <div class="flex flex-col gap-1.5">
                        <Label htmlFor="oc-protocol" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Server Protocol</Label>
                        <select id="oc-protocol" v-model="form.protocol_hint"
                            class="flex h-9 w-full rounded-md border px-3 py-1 text-sm disabled:cursor-not-allowed disabled:opacity-50" 
                            :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-white focus:!outline-none focus:!ring-2 focus:!ring-[#2D8CFF]/40 !ring-offset-0' : 'bg-background border-input focus:outline-none focus:ring-2 focus:ring-ring'">
                            <option v-for="p in openConnectProtocols" :key="p.value" :value="p.value">
                                {{ p.label }} — {{ p.description }}
                            </option>
                        </select>
                        <p class="text-xs" :class="settingsStore.appTheme === 'glass' ? 'text-white/40' : 'text-muted-foreground'">Choose Auto-detect if unsure — OpenConnect will probe the server.</p>
                    </div>

                    <!-- Username + Password -->
                    <div class="grid grid-cols-2 gap-3">
                        <div class="flex flex-col gap-1.5">
                            <Label htmlFor="oc-user" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Username</Label>
                            <Input id="oc-user" v-model="form.username" placeholder="john.doe" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                        </div>
                        <div class="flex flex-col gap-1.5">
                            <Label htmlFor="oc-pass" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Password</Label>
                            <Input id="oc-pass" type="password" v-model="form.password"
                                :placeholder="isEditing ? 'Leave blank to keep' : 'Password'" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                        </div>
                    </div>

                    <!-- 2FA note -->
                    <div class="rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-2.5 text-xs text-amber-400 leading-relaxed flex items-start gap-2" :class="settingsStore.appTheme === 'glass' ? '!bg-[#1A1605] !border-amber-500/20' : ''">
                        <Lock class="w-3.5 h-3.5 flex-shrink-0 mt-0.5" />
                        <span>If your VPN requires a 2FA / MFA token, a prompt will appear automatically when you connect. You do not need to configure it here.</span>
                    </div>
                </template>

                <!-- Validation error -->
                <div v-if="formError" class="text-xs text-red-400 flex items-center gap-1.5 flex-shrink-0">
                    <X class="w-3.5 h-3.5 flex-shrink-0" />
                    {{ formError }}
                </div>
            </div>

            <DialogFooter class="mt-4 border-t pt-4 flex-shrink-0" :class="settingsStore.appTheme === 'glass' ? 'border-white/10' : 'border-border'">
                <Button variant="outline" @click="isDialogOpen = false" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">Cancel</Button>
                <Button 
                    variant="default"
                    @click="saveProfile" 
                    :disabled="!isFormValid || isSaving"
                    :class="settingsStore.appTheme === 'glass' ? '!bg-gradient-to-br !from-[#1DE9B6] !to-[#2D8CFF] !text-[#062018] !border-none hover:!opacity-90' : ''"
                >
                    <Loader2 v-if="isSaving" class="w-4 h-4 mr-2 animate-spin" />
                    {{ isEditing ? 'Save Changes' : 'Save profile' }}
                </Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>

    <!-- ── 2FA Token Dialog ───────────────────────────────────────────────── -->
    <Dialog v-model:open="isMfaDialogOpen">
        <DialogContent class="sm:max-w-[380px]">
            <DialogHeader>
                <DialogTitle class="flex items-center gap-2">
                    <Lock class="w-4 h-4 text-amber-400" />
                    Two-Factor Authentication
                </DialogTitle>
                <DialogDescription>
                    {{ mfaPrompt }}
                </DialogDescription>
            </DialogHeader>
            <div class="py-3 flex flex-col gap-3">
                <div class="flex flex-col gap-1.5">
                    <Label htmlFor="mfa-token">Authentication Token</Label>
                    <Input
                        id="mfa-token"
                        v-model="mfaToken"
                        placeholder="123456"
                        class="font-mono text-center text-lg tracking-widest"
                        @keyup.enter="submitMfaToken"
                        autofocus
                    />
                </div>
                <p class="text-xs text-muted-foreground">
                    Enter the one-time code from your authenticator app (Duo, Google Authenticator, etc.)
                </p>
            </div>
            <DialogFooter>
                <Button variant="outline" @click="isMfaDialogOpen = false">Cancel</Button>
                <Button @click="submitMfaToken" :disabled="!mfaToken.trim() || mfaSubmitting" class="gap-2">
                    <Loader2 v-if="mfaSubmitting" class="w-3.5 h-3.5 animate-spin" />
                    Submit Token
                </Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>

    <!-- ── Delete Confirm Dialog ─────────────────────────────────────────── -->
    <Dialog :open="!!confirmDeleteId" @update:open="v => { if (!v) confirmDeleteId = null }">
        <DialogContent class="sm:max-w-[360px]">
            <DialogHeader>
                <DialogTitle class="flex items-center gap-2 text-red-400">
                    <Trash2 class="w-4 h-4" />
                    Delete Profile
                </DialogTitle>
                <DialogDescription>
                    This VPN profile will be permanently deleted. This cannot be undone.
                </DialogDescription>
            </DialogHeader>
            <DialogFooter class="mt-4">
                <Button variant="outline" @click="confirmDeleteId = null">Cancel</Button>
                <Button variant="destructive" @click="doDelete" class="gap-2">
                    <Trash2 class="w-3.5 h-3.5" />
                    Delete
                </Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>

    <!-- ── Trust Certificate Dialog ──────────────────────────────────────── -->
    <Dialog v-model:open="isCertDialogOpen">
        <DialogContent class="sm:max-w-[420px]">
            <DialogHeader>
                <DialogTitle class="flex items-center gap-2 text-amber-400">
                    <Shield class="w-4 h-4" />
                    Untrusted Certificate
                </DialogTitle>
                <DialogDescription>
                    The VPN server <strong class="text-foreground">{{ certServer }}</strong> is using a self-signed certificate that could not be verified.
                </DialogDescription>
            </DialogHeader>
            <div class="py-3 space-y-3">
                <div class="rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-2.5 text-xs text-amber-400 leading-relaxed">
                    <p class="font-semibold mb-1">Certificate Fingerprint</p>
                    <p class="font-mono break-all text-[11px] text-foreground/80">{{ certFingerprint }}</p>
                </div>
                <p class="text-xs text-muted-foreground">
                    Only trust this certificate if you recognise this server and its fingerprint. Once trusted, Airlock will automatically accept it on future connections.
                </p>
            </div>
            <DialogFooter>
                <Button variant="outline" @click="isCertDialogOpen = false">Cancel</Button>
                <Button @click="trustCertAndReconnect" :disabled="certTrusting" class="gap-2 bg-amber-500 hover:bg-amber-600 text-black">
                    <Loader2 v-if="certTrusting" class="w-3.5 h-3.5 animate-spin" />
                    Trust &amp; Connect
                </Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>

  </div>
</template>
