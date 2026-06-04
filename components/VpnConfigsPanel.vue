<script setup lang="ts">
import { ref, computed } from 'vue'
import { useVpnConfigsStore } from '~/stores/vpn_configs'
import { useVpnStore } from '~/stores/vpn'
import {
    Shield, ShieldCheck, ShieldOff, ShieldAlert, Loader2,
    Plus, Pencil, Trash2, ChevronRight, Wifi, WifiOff, X
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

const vpnConfigsStore = useVpnConfigsStore()
const vpnStore = useVpnStore()

// ── Dialog state ─────────────────────────────────────────────────────────────
const isDialogOpen = ref(false)
const editingId = ref<string | null>(null)
const confirmDeleteId = ref<string | null>(null)

const form = ref({
    name: '',
    config: '',
})
const formError = ref('')
const isSaving = ref(false)

// ── Connection state ──────────────────────────────────────────────────────────
const connectingId = ref<string | null>(null)
const connectionError = ref('')

// ── Computed ──────────────────────────────────────────────────────────────────
const isEditing = computed(() => editingId.value !== null)

const activeProfileId = computed(() => {
    // We track which profile is active by matching against the store status
    return vpnStore.status === 'connected' ? (vpnStore as any).activeProfileId ?? null : null
})

const statusColor = computed(() => {
    switch (vpnStore.status) {
        case 'connected': return 'text-green-400'
        case 'connecting': return 'text-amber-400'
        case 'error': return 'text-red-400'
        default: return 'text-zinc-400'
    }
})

const statusDotClass = computed(() => {
    switch (vpnStore.status) {
        case 'connected': return 'bg-green-500'
        case 'connecting': return 'bg-amber-400 animate-pulse'
        case 'error': return 'bg-red-500'
        default: return 'bg-zinc-500'
    }
})

const statusLabel = computed(() => {
    switch (vpnStore.status) {
        case 'connected': return 'Connected'
        case 'connecting': return 'Connecting...'
        case 'error': return 'Error'
        default: return 'Disconnected'
    }
})

// ── Methods ───────────────────────────────────────────────────────────────────
function openAddDialog() {
    editingId.value = null
    form.value = { name: '', config: '' }
    formError.value = ''
    isDialogOpen.value = true
}

async function openEditDialog(id: string) {
    editingId.value = id
    const profile = vpnConfigsStore.profiles.find(p => p.id === id)
    if (!profile) return
    const decrypted = await vpnConfigsStore.getDecryptedConfig(id)
    form.value = { name: profile.name, config: decrypted }
    formError.value = ''
    isDialogOpen.value = true
}

async function saveProfile() {
    formError.value = ''
    if (!form.value.name.trim()) {
        formError.value = 'Profile name is required.'
        return
    }
    if (!form.value.config.trim()) {
        formError.value = 'WireGuard configuration is required.'
        return
    }
    // Basic validation — must have [Interface] and [Peer]
    if (!form.value.config.includes('[Interface]') || !form.value.config.includes('[Peer]')) {
        formError.value = 'Invalid WireGuard config — must contain [Interface] and [Peer] sections.'
        return
    }

    isSaving.value = true
    try {
        if (isEditing.value && editingId.value) {
            await vpnConfigsStore.updateProfile(editingId.value, form.value.name.trim(), form.value.config.trim())
        } else {
            await vpnConfigsStore.addProfile(form.value.name.trim(), form.value.config.trim())
        }
        isDialogOpen.value = false
    } finally {
        isSaving.value = false
    }
}

function confirmDelete(id: string) {
    confirmDeleteId.value = id
}

function doDelete() {
    if (confirmDeleteId.value) {
        // If this profile is connected, disconnect first
        if (vpnStore.status === 'connected') {
            vpnStore.disconnect().catch(() => {})
        }
        vpnConfigsStore.removeProfile(confirmDeleteId.value)
        confirmDeleteId.value = null
    }
}

async function connectProfile(id: string) {
    if (vpnStore.status === 'connected') {
        // Disconnect current first
        await vpnStore.disconnect().catch(() => {})
    }
    connectingId.value = id
    connectionError.value = ''
    try {
        const config = await vpnConfigsStore.getDecryptedConfig(id)
        if (!config) throw new Error('Failed to decrypt VPN configuration')
        ;(vpnStore as any).activeProfileId = id
        await vpnStore.connect(config)
    } catch (e: any) {
        connectionError.value = typeof e === 'string' ? e : (e?.message ?? 'Connection failed')
    } finally {
        connectingId.value = null
    }
}

async function disconnect() {
    connectionError.value = ''
    try {
        await vpnStore.disconnect()
        ;(vpnStore as any).activeProfileId = null
    } catch (e: any) {
        connectionError.value = typeof e === 'string' ? e : (e?.message ?? 'Disconnect failed')
    }
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

    <!-- ── Global VPN Status Banner ───────────────────────────────────────── -->
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
                v-if="vpnStore.status === 'connected'"
                @click="disconnect"
                class="text-xs px-2 py-0.5 rounded border border-red-500/40 text-red-400 hover:bg-red-500/10 transition-colors"
            >
                Disconnect
            </button>
        </div>
        <!-- Error message -->
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
                <p class="text-xs text-muted-foreground mt-0.5">Add a WireGuard config to get started</p>
            </div>
            <button
                @click="openAddDialog"
                class="text-xs px-3 py-1.5 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 transition-colors"
            >
                Add Profile
            </button>
        </div>

        <!-- Profile cards -->
        <div
            v-for="profile in vpnConfigsStore.profiles"
            :key="profile.id"
            class="group rounded-lg border transition-all"
            :class="{
                'border-green-500/50 bg-green-500/5': vpnStore.status === 'connected' && (vpnStore as any).activeProfileId === profile.id,
                'border-border bg-muted/20 hover:border-border/80 hover:bg-muted/30': !(vpnStore.status === 'connected' && (vpnStore as any).activeProfileId === profile.id),
            }"
        >
            <div class="flex items-center gap-3 px-3 py-3">
                <!-- Icon -->
                <div class="flex-shrink-0">
                    <ShieldCheck
                        v-if="vpnStore.status === 'connected' && (vpnStore as any).activeProfileId === profile.id"
                        class="w-5 h-5 text-green-400"
                    />
                    <Shield v-else class="w-5 h-5 text-muted-foreground group-hover:text-foreground transition-colors" />
                </div>

                <!-- Name -->
                <div class="flex-1 min-w-0">
                    <p class="text-sm font-medium truncate">{{ profile.name }}</p>
                    <p class="text-xs text-muted-foreground">WireGuard · Encrypted</p>
                </div>

                <!-- Actions -->
                <div class="flex items-center gap-1 flex-shrink-0">
                    <!-- Connect / Disconnect button -->
                    <button
                        v-if="vpnStore.status === 'connected' && (vpnStore as any).activeProfileId === profile.id"
                        @click="disconnect"
                        class="text-xs px-2.5 py-1 rounded-md border border-red-500/40 text-red-400 hover:bg-red-500/10 transition-colors"
                    >
                        Disconnect
                    </button>
                    <button
                        v-else
                        @click="connectProfile(profile.id)"
                        :disabled="connectingId === profile.id || vpnStore.status === 'connecting'"
                        class="text-xs px-2.5 py-1 rounded-md border border-primary/40 text-primary hover:bg-primary/10 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1"
                    >
                        <Loader2 v-if="connectingId === profile.id" class="w-3 h-3 animate-spin" />
                        <Wifi v-else class="w-3 h-3" />
                        {{ connectingId === profile.id ? 'Connecting...' : 'Connect' }}
                    </button>

                    <!-- Edit -->
                    <button
                        @click="openEditDialog(profile.id)"
                        class="p-1.5 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                        title="Edit profile"
                    >
                        <Pencil class="w-3.5 h-3.5" />
                    </button>

                    <!-- Delete -->
                    <button
                        @click="confirmDelete(profile.id)"
                        class="p-1.5 rounded-md text-muted-foreground hover:text-red-400 hover:bg-red-500/10 transition-colors"
                        title="Delete profile"
                    >
                        <Trash2 class="w-3.5 h-3.5" />
                    </button>
                </div>
            </div>
        </div>
    </div>

    <!-- ── Add / Edit Dialog ──────────────────────────────────────────────── -->
    <Dialog v-model:open="isDialogOpen">
        <DialogContent class="sm:max-w-[520px] max-h-[90vh] flex flex-col">
            <DialogHeader class="flex-shrink-0">
                <DialogTitle class="flex items-center gap-2">
                    <Shield class="w-4 h-4 text-primary" />
                    {{ isEditing ? 'Edit VPN Profile' : 'Add VPN Profile' }}
                </DialogTitle>
                <DialogDescription>
                    Paste your WireGuard <code class="bg-muted px-1 rounded text-xs">.conf</code> file content below.
                    Your config is encrypted with AES-256 before being stored.
                </DialogDescription>
            </DialogHeader>

            <div class="flex flex-col gap-4 py-2 flex-1 overflow-y-auto">
                <!-- Profile name -->
                <div class="flex flex-col gap-1.5">
                    <Label htmlFor="vpn-profile-name">Profile Name</Label>
                    <Input
                        id="vpn-profile-name"
                        v-model="form.name"
                        placeholder="e.g. Work VPN, Home Server, Mullvad US"
                        @keyup.enter="saveProfile"
                    />
                </div>

                <!-- WireGuard config -->
                <div class="flex flex-col gap-1.5 flex-1">
                    <div class="flex items-center justify-between">
                        <Label htmlFor="vpn-config-text">WireGuard Configuration</Label>
                        <span class="text-xs text-muted-foreground">Paste your .conf file</span>
                    </div>
                    <textarea
                        id="vpn-config-text"
                        v-model="form.config"
                        rows="12"
                        placeholder="[Interface]
PrivateKey = <your-private-key>
Address = 10.8.0.2/24
DNS = 1.1.1.1

[Peer]
PublicKey = <server-public-key>
Endpoint = vpn.example.com:51820
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25"
                        class="w-full rounded-md border border-input bg-background px-3 py-2 text-xs font-mono resize-none ring-offset-background focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 text-foreground placeholder:text-muted-foreground/50 leading-relaxed"
                    />
                    <div class="flex items-start gap-1.5 text-xs text-muted-foreground">
                        <ShieldCheck class="w-3.5 h-3.5 text-green-500 flex-shrink-0 mt-0.5" />
                        <span>Encrypted with AES-256-GCM before being stored. Never saved in plaintext.</span>
                    </div>
                    <div v-if="formError" class="text-xs text-red-400 flex items-center gap-1.5">
                        <X class="w-3.5 h-3.5 flex-shrink-0" />
                        {{ formError }}
                    </div>
                </div>

                <!-- Key requirement notice -->
                <div class="rounded-md border border-amber-500/30 bg-amber-500/5 px-3 py-2.5 text-xs text-amber-400 leading-relaxed flex-shrink-0">
                    <strong>WireGuard requires cryptographic keys.</strong> Your VPN provider (Mullvad, ProtonVPN, etc.)
                    or server admin will give you a ready-to-use <code class="bg-amber-500/20 px-1 rounded">.conf</code> file
                    — simply paste it above. There is no way to connect to WireGuard with just an IP and port.
                </div>
            </div>

            <DialogFooter class="flex-shrink-0 pt-2">
                <Button variant="outline" @click="isDialogOpen = false">Cancel</Button>
                <Button @click="saveProfile" :disabled="isSaving" class="gap-2">
                    <Loader2 v-if="isSaving" class="w-3.5 h-3.5 animate-spin" />
                    {{ isEditing ? 'Update' : 'Save Profile' }}
                </Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>

    <!-- ── Delete Confirmation Dialog ────────────────────────────────────── -->
    <Dialog :open="!!confirmDeleteId" @update:open="v => { if (!v) confirmDeleteId = null }">
        <DialogContent class="sm:max-w-[360px]">
            <DialogHeader>
                <DialogTitle class="flex items-center gap-2 text-red-400">
                    <Trash2 class="w-4 h-4" />
                    Delete Profile
                </DialogTitle>
                <DialogDescription>
                    Are you sure? This VPN profile will be permanently deleted. This action cannot be undone.
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

  </div>
</template>
