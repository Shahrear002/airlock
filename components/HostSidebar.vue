<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useHostsStore } from '~/stores/hosts'
import { useVpnStore } from '~/stores/vpn'
import {
    Plus, FolderPlus, Terminal as TerminalIcon, Cog,
    Shield, ShieldCheck, ShieldOff, Loader2, ServerIcon
} from 'lucide-vue-next'
import Dialog from '@/components/ui/dialog/Dialog.vue'
import DialogContent from '@/components/ui/dialog/DialogContent.vue'
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue'
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue'
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue'
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue'
import Button from '@/components/ui/button/Button.vue'
import Input from '@/components/ui/input/Input.vue'
import Label from '@/components/ui/label/Label.vue'
import SidebarTreeItem from './SidebarTreeItem.vue'
import SettingsDialog from './SettingsDialog.vue'
import VpnConfigsPanel from './VpnConfigsPanel.vue'
import { TauriStoreAdapter } from '~/utils/store-adapter'
import { useSettingsStore } from '~/stores/settings'

const hostsStore = useHostsStore()
const vpnStore = useVpnStore()
const settingsStore = useSettingsStore()

// ── Active tab ────────────────────────────────────────────────────────────────
const activeTab = ref<'hosts' | 'vpn'>('hosts')

// ── Dialog state ──────────────────────────────────────────────────────────────
const isAddModalOpen = ref(false)
const isFolderModalOpen = ref(false)
const isRenameModalOpen = ref(false)
const isSettingsOpen = ref(false)
const editingHostId = ref<string | null>(null)
const editingFolderId = ref<string | null>(null)

const newHost = ref({
    name: '',
    host: '',
    port: 22,
    username: '',
    password: '',
    parentId: null as string | null,
})

const newFolder = ref({ name: '', parentId: null as string | null })
const renameData = ref({ name: '' })

const emit = defineEmits(['connect'])

onMounted(async () => {
    if (hostsStore.hosts.length === 0) {
        const raw = await TauriStoreAdapter.getItem('hosts')
        if (raw) {
            try {
                const parsed = JSON.parse(raw)
                if (parsed.hosts) hostsStore.$patch({ hosts: parsed.hosts })
            } catch (e) {
                console.error('HostSidebar: Manual patch failed:', e)
            }
        }
    }
})

// ── Computed ──────────────────────────────────────────────────────────────────
const rootItems = computed(() => hostsStore.getChildren(null))
const allFolders = computed(() => hostsStore.hosts.filter(h => h.type === 'folder'))

const getFolderPath = (folder: any): string => {
    if (!folder.parentId) return folder.name
    const parent = hostsStore.hosts.find(h => h.id === folder.parentId)
    return parent ? `${getFolderPath(parent)} > ${folder.name}` : folder.name
}

const formattedFolders = computed(() =>
    allFolders.value
        .map(f => ({ ...f, displayName: getFolderPath(f) }))
        .sort((a, b) => a.displayName.localeCompare(b.displayName))
)

// VPN status indicator for the tab badge
const vpnStatusDot = computed(() => {
    switch (vpnStore.status) {
        case 'connected': return 'bg-green-500'
        case 'connecting': return 'bg-amber-400 animate-pulse'
        case 'error': return 'bg-red-500'
        default: return 'bg-zinc-600'
    }
})

// ── Host CRUD ─────────────────────────────────────────────────────────────────
function resetForms() {
    editingHostId.value = null
    editingFolderId.value = null
    newHost.value = { name: '', host: '', port: 22, username: '', password: '', parentId: null }
    newFolder.value = { name: '', parentId: null }
}

async function saveHost() {
    if (!newHost.value.name || !newHost.value.host || !newHost.value.username) return
    if (editingHostId.value) {
        await hostsStore.updateHost(editingHostId.value, {
            name: newHost.value.name,
            host: newHost.value.host,
            port: newHost.value.port,
            username: newHost.value.username,
            password: newHost.value.password || undefined,
            parentId: newHost.value.parentId,
        })
    } else {
        await hostsStore.addHost({
            name: newHost.value.name,
            host: newHost.value.host,
            port: newHost.value.port,
            username: newHost.value.username,
            password: newHost.value.password,
            parentId: newHost.value.parentId,
        })
    }
    resetForms()
    isAddModalOpen.value = false
}

function saveFolder() {
    if (!newFolder.value.name) return
    hostsStore.addFolder(newFolder.value.name, newFolder.value.parentId)
    resetForms()
    isFolderModalOpen.value = false
}

function saveRename() {
    if (editingFolderId.value && renameData.value.name) {
        hostsStore.updateFolder(editingFolderId.value, renameData.value.name)
        isRenameModalOpen.value = false
        editingFolderId.value = null
        renameData.value.name = ''
    }
}

async function onEditHost(host: any) {
    editingHostId.value = host.id
    newHost.value = {
        name: host.name,
        host: host.host || '',
        port: host.port || 22,
        username: host.username || '',
        password: '',
        parentId: host.parentId,
    }
    isAddModalOpen.value = true
}

function onRenameFolder(folder: any) {
    editingFolderId.value = folder.id
    renameData.value.name = folder.name
    isRenameModalOpen.value = true
}

function onCreateHostInFolder(folderId: string) {
    resetForms()
    newHost.value.parentId = folderId
    isAddModalOpen.value = true
}

function onCreateFolderInFolder(folderId: string) {
    resetForms()
    newFolder.value.parentId = folderId
    isFolderModalOpen.value = true
}

async function connectToHost(hostId: string) {
    const host = hostsStore.hosts.find(h => h.id === hostId)
    if (!host || host.type !== 'host') return
    const decryptedPassword = await hostsStore.getDecryptedPassword(hostId)
    emit('connect', { ...host, password: decryptedPassword })
}
</script>

<template>
  <div 
    class="flex flex-col h-full w-full transition-colors duration-300"
    :class="settingsStore.appTheme === 'glass' ? 'glass-panel' : 'bg-card border-r border-border overflow-hidden'"
  >

    <!-- ── Tabs ───────────────────────────────────────────────────────────── -->
    <div class="flex gap-1 px-3 pb-2 pt-2 flex-shrink-0">
        <button
            @click="activeTab = 'hosts'"
            class="flex-1 flex items-center justify-center gap-1.5 py-1.5 rounded-md text-xs font-medium transition-colors"
            :class="[
                activeTab === 'hosts' && settingsStore.appTheme !== 'glass' ? 'bg-muted text-foreground' : '',
                activeTab !== 'hosts' && settingsStore.appTheme !== 'glass' ? 'text-muted-foreground hover:text-foreground hover:bg-muted/50' : '',
                activeTab === 'hosts' && settingsStore.appTheme === 'glass' ? 'bg-white/10 text-white' : '',
                activeTab !== 'hosts' && settingsStore.appTheme === 'glass' ? 'text-white/40 hover:bg-white/5 hover:text-white' : ''
            ]"
        >
            <ServerIcon class="w-3.5 h-3.5" />
            Hosts
        </button>
        <button
            @click="activeTab = 'vpn'"
            class="flex-1 flex items-center justify-center gap-1.5 py-1.5 rounded-md text-xs font-medium transition-colors relative"
            :class="[
                activeTab === 'vpn' && settingsStore.appTheme !== 'glass' ? 'bg-muted text-foreground' : '',
                activeTab !== 'vpn' && settingsStore.appTheme !== 'glass' ? 'text-muted-foreground hover:text-foreground hover:bg-muted/50' : '',
                activeTab === 'vpn' && settingsStore.appTheme === 'glass' ? 'bg-white/10 text-white' : '',
                activeTab !== 'vpn' && settingsStore.appTheme === 'glass' ? 'text-white/40 hover:bg-white/5 hover:text-white' : ''
            ]"
        >
            <Shield class="w-3.5 h-3.5" />
            VPN
            <!-- Status dot on VPN tab -->
            <span
                class="absolute top-1 right-2 w-1.5 h-1.5 rounded-full"
                :class="vpnStatusDot"
            />
        </button>
    </div>

    <!-- ── Hosts Panel ────────────────────────────────────────────────────── -->
    <div v-if="activeTab === 'hosts'" class="flex flex-col flex-1 min-h-0">

        <!-- Tree -->
        <div class="flex-1 overflow-y-auto px-2">
            <div v-if="hostsStore.hosts.length === 0" class="text-xs text-muted-foreground italic px-1 py-2">
                No hosts saved.
            </div>
            <div class="space-y-0.5">
                <SidebarTreeItem
                    v-for="item in rootItems"
                    :key="item.id"
                    :item="item"
                    :depth="0"
                    @connect="connectToHost"
                    @edit="onEditHost"
                    @rename="onRenameFolder"
                    @create-host="onCreateHostInFolder"
                    @create-folder="onCreateFolderInFolder"
                />
            </div>
        </div>

        <!-- Footer actions -->
        <div class="border-t px-3 py-2 flex gap-1.5 flex-shrink-0" :class="settingsStore.appTheme === 'glass' ? 'border-white/10' : 'border-border'">
            <!-- Add Host -->
            <Dialog v-model:open="isAddModalOpen">
                <button
                    @click="resetForms(); isAddModalOpen = true"
                    class="flex-1 flex items-center justify-center gap-1 text-xs py-1.5 px-2 rounded-md border transition-colors"
                    :class="settingsStore.appTheme === 'glass' ? 'border-white/10 hover:bg-white/5 text-white/80 hover:text-white' : 'border-border hover:bg-muted'"
                >
                    <Plus class="w-3.5 h-3.5" />
                    Host
                </button>
                <DialogContent class="sm:max-w-[440px]" :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-[#E9EDF1] shadow-[0_0_80px_rgba(0,0,0,0.8)]' : ''">
                    <DialogHeader>
                        <DialogTitle :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]' : ''">{{ editingHostId ? 'Edit Host' : 'Add SSH Host' }}</DialogTitle>
                        <DialogDescription :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/60' : ''">
                            Credentials are encrypted locally before being stored.
                        </DialogDescription>
                    </DialogHeader>

                    <div class="flex flex-col gap-3 py-3">
                        <!-- Folder -->
                        <div class="flex flex-col gap-1.5">
                            <Label htmlFor="hf-folder" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Folder</Label>
                            <select id="hf-folder" v-model="newHost.parentId"
                                class="flex h-9 w-full rounded-md border px-3 py-1 text-sm disabled:cursor-not-allowed disabled:opacity-50"
                                :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-white focus:!outline-none focus:!ring-2 focus:!ring-[#2D8CFF]/40' : 'border-input bg-background focus:outline-none focus:ring-2 focus:ring-ring'">
                                <option :value="null">Root (None)</option>
                                <option v-for="f in formattedFolders" :key="f.id" :value="f.id">{{ f.displayName }}</option>
                            </select>
                        </div>

                        <!-- Label -->
                        <div class="flex flex-col gap-1.5">
                            <Label htmlFor="hf-name" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Label</Label>
                            <Input id="hf-name" v-model="newHost.name" placeholder="Production Server" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                        </div>

                        <!-- Host + Port side by side -->
                        <div class="flex gap-3">
                            <div class="flex flex-col gap-1.5 flex-1">
                                <Label htmlFor="hf-host" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Host / IP</Label>
                                <Input id="hf-host" v-model="newHost.host" placeholder="192.168.1.1" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                            </div>
                            <div class="flex flex-col gap-1.5 w-20">
                                <Label htmlFor="hf-port" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Port</Label>
                                <Input id="hf-port" type="number" v-model="newHost.port" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                            </div>
                        </div>

                        <!-- Username + Password side by side -->
                        <div class="flex gap-3">
                            <div class="flex flex-col gap-1.5 flex-1">
                                <Label htmlFor="hf-user" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Username</Label>
                                <Input id="hf-user" v-model="newHost.username" placeholder="root" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                            </div>
                            <div class="flex flex-col gap-1.5 flex-1">
                                <Label htmlFor="hf-pass" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Password</Label>
                                <Input id="hf-pass" type="password" v-model="newHost.password"
                                    :placeholder="editingHostId ? 'Leave blank to keep' : 'Password'" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                            </div>
                        </div>
                    </div>

                    <DialogFooter :class="settingsStore.appTheme === 'glass' ? 'border-none' : ''">
                        <Button variant="outline" @click="isAddModalOpen = false" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">Cancel</Button>
                        <Button @click="saveHost" :class="settingsStore.appTheme === 'glass' ? 'btn-primary' : ''">{{ editingHostId ? 'Update' : 'Save' }}</Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>

            <!-- Add Folder -->
            <Dialog v-model:open="isFolderModalOpen">
                <button
                    @click="resetForms(); isFolderModalOpen = true"
                    class="p-1.5 rounded-md border transition-colors"
                    :class="settingsStore.appTheme === 'glass' ? 'border-white/10 hover:bg-white/5 text-white/80 hover:text-white' : 'border-border hover:bg-muted'"
                    title="New Folder"
                >
                    <FolderPlus class="w-4 h-4" />
                </button>
                <DialogContent class="sm:max-w-[360px]" :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-[#E9EDF1] shadow-[0_0_80px_rgba(0,0,0,0.8)]' : ''">
                    <DialogHeader>
                        <DialogTitle :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]' : ''">New Folder</DialogTitle>
                        <DialogDescription :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/60' : ''">Organize your hosts into folders.</DialogDescription>
                    </DialogHeader>
                    <div class="flex flex-col gap-3 py-3">
                        <div class="flex flex-col gap-1.5">
                            <Label htmlFor="ff-parent" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Parent</Label>
                            <select id="ff-parent" v-model="newFolder.parentId"
                                class="flex h-9 w-full rounded-md border px-3 py-1 text-sm disabled:cursor-not-allowed disabled:opacity-50"
                                :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-white focus:!outline-none focus:!ring-2 focus:!ring-[#2D8CFF]/40' : 'border-input bg-background focus:outline-none focus:ring-2 focus:ring-ring'">
                                <option :value="null">Root (None)</option>
                                <option v-for="f in formattedFolders" :key="f.id" :value="f.id">{{ f.displayName }}</option>
                            </select>
                        </div>
                        <div class="flex flex-col gap-1.5">
                            <Label htmlFor="ff-name" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/70' : ''">Name</Label>
                            <Input id="ff-name" v-model="newFolder.name" placeholder="My Project" @keyup.enter="saveFolder" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                        </div>
                    </div>
                    <DialogFooter :class="settingsStore.appTheme === 'glass' ? 'border-none' : ''">
                        <Button variant="outline" @click="isFolderModalOpen = false" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">Cancel</Button>
                        <Button @click="saveFolder" :class="settingsStore.appTheme === 'glass' ? 'btn-primary' : ''">Create</Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>

            <!-- Settings -->
            <button
                @click="isSettingsOpen = true"
                class="p-1.5 rounded-md border transition-colors"
                :class="settingsStore.appTheme === 'glass' ? 'border-white/10 hover:bg-white/5 text-white/80 hover:text-white' : 'border-border hover:bg-muted'"
                title="Settings"
            >
                <Cog class="w-4 h-4" />
            </button>
        </div>
    </div>

    <!-- ── VPN Panel ──────────────────────────────────────────────────────── -->
    <div v-else-if="activeTab === 'vpn'" class="flex-1 min-h-0 overflow-hidden">
        <VpnConfigsPanel />
    </div>

    <!-- ── Rename Folder Dialog (shared) ─────────────────────────────────── -->
    <Dialog v-model:open="isRenameModalOpen">
        <DialogContent class="sm:max-w-[360px]" :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-[#E9EDF1] shadow-[0_0_80px_rgba(0,0,0,0.8)]' : ''">
            <DialogHeader>
                <DialogTitle :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]' : ''">Rename Folder</DialogTitle>
            </DialogHeader>
            <div class="py-3">
                <Input v-model="renameData.name" @keyup.enter="saveRename" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
            </div>
            <DialogFooter :class="settingsStore.appTheme === 'glass' ? 'border-none' : ''">
                <Button variant="outline" @click="isRenameModalOpen = false" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">Cancel</Button>
                <Button @click="saveRename" :class="settingsStore.appTheme === 'glass' ? 'btn-primary' : ''">Rename</Button>
            </DialogFooter>
        </DialogContent>
    </Dialog>

    <SettingsDialog v-model:open="isSettingsOpen" />
  </div>
</template>
