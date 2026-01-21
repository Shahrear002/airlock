<script setup lang="ts">
import { ref, computed } from 'vue'
import { useHostsStore } from '~/stores/hosts'
import { Plus, FolderPlus, Terminal as TerminalIcon, Cog } from 'lucide-vue-next'
import Dialog from '@/components/ui/dialog/Dialog.vue'
import DialogContent from '@/components/ui/dialog/DialogContent.vue'
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue'
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue'
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue'
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue'
import DialogTrigger from '@/components/ui/dialog/DialogTrigger.vue'
import Button from '@/components/ui/button/Button.vue'
import Input from '@/components/ui/input/Input.vue'
import Label from '@/components/ui/label/Label.vue'
import SidebarTreeItem from './SidebarTreeItem.vue'
import SettingsDialog from './SettingsDialog.vue'

const hostsStore = useHostsStore()
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
    parentId: null as string | null
})

const newFolder = ref({
    name: '',
    parentId: null as string | null
})

const renameData = ref({
    name: ''
})

import { TauriStoreAdapter } from '~/utils/store-adapter'

const emit = defineEmits(['connect'])

onMounted(async () => {
    // Manual Hydration Fallback
    // If Pinia fails to rehydrate from Tauri store automatically (race condition), do it manually.
    if (hostsStore.hosts.length === 0) {
        const raw = await TauriStoreAdapter.getItem('hosts')
        if (raw) {
            try {
                const parsed = JSON.parse(raw)
                if (parsed.hosts) {
                    hostsStore.$patch({ hosts: parsed.hosts })
                }
            } catch (e) {
                console.error('HostSidebar: Manual patch failed:', e)
            }
        }
    }
})

const rootItems = computed(() => hostsStore.getChildren(null))
const allFolders = computed(() => hostsStore.hosts.filter(h => h.type === 'folder'))

// Helper to get full path
const getFolderPath = (folder: any): string => {
    if (!folder.parentId) return folder.name
    const parent = hostsStore.hosts.find(h => h.id === folder.parentId)
    return parent ? `${getFolderPath(parent)} > ${folder.name}` : folder.name
}

const formattedFolders = computed(() => {
    return allFolders.value.map(f => ({
        ...f,
        displayName: getFolderPath(f)
    })).sort((a, b) => a.displayName.localeCompare(b.displayName))
})

const saveHost = async () => {
    if (!newHost.value.name || !newHost.value.host || !newHost.value.username) return

    if (editingHostId.value) {
         await hostsStore.updateHost(editingHostId.value, {
            name: newHost.value.name,
            host: newHost.value.host,
            port: newHost.value.port,
            username: newHost.value.username,
            password: newHost.value.password || undefined,
            parentId: newHost.value.parentId
        })
    } else {
        await hostsStore.addHost({
            name: newHost.value.name,
            host: newHost.value.host,
            port: newHost.value.port,
            username: newHost.value.username,
            password: newHost.value.password,
            parentId: newHost.value.parentId
        })
    }

    resetForms()
    isAddModalOpen.value = false
}

const saveFolder = () => {
    if (!newFolder.value.name) return
    hostsStore.addFolder(newFolder.value.name, newFolder.value.parentId)
    resetForms()
    isFolderModalOpen.value = false
}

const saveRename = () => {
    if (editingFolderId.value && renameData.value.name) {
        hostsStore.updateFolder(editingFolderId.value, renameData.value.name)
        isRenameModalOpen.value = false
        editingFolderId.value = null
        renameData.value.name = ''
    }
}

const onEditHost = async (host: any) => {
    editingHostId.value = host.id
    newHost.value = {
        name: host.name,
        host: host.host || '',
        port: host.port || 22,
        username: host.username || '',
        password: '', // Don't fill password
        parentId: host.parentId
    }
    isAddModalOpen.value = true
}

const onRenameFolder = (folder: any) => {
    editingFolderId.value = folder.id
    renameData.value.name = folder.name
    isRenameModalOpen.value = true
}

const onCreateHostInFolder = (folderId: string) => {
    resetForms()
    newHost.value.parentId = folderId
    isAddModalOpen.value = true
}

const onCreateFolderInFolder = (folderId: string) => {
    resetForms()
    newFolder.value.parentId = folderId
    isFolderModalOpen.value = true
}

const resetForms = () => {
    editingHostId.value = null
    editingFolderId.value = null
    newHost.value = {
        name: '',
        host: '',
        port: 22,
        username: '',
        password: '',
        parentId: null
    }
    newFolder.value = {
        name: '',
        parentId: null
    }
}

const connectToHost = async (hostId: string) => {
  const host = hostsStore.hosts.find(h => h.id === hostId)
  if (!host || host.type !== 'host') return
  
  const decryptedPassword = await hostsStore.getDecryptedPassword(hostId)
  
  emit('connect', {
    ...host,
    password: decryptedPassword
  })
}
</script>

<template>
  <div class="w-64 border-r border-border bg-card p-4 flex flex-col gap-4 h-full">
    <div class="flex items-center gap-2 text-xl font-bold tracking-tight text-primary">
        <TerminalIcon class="w-6 h-6" />
        Airlock
    </div>
    
    <div class="flex-1 overflow-y-auto">
      <div class="flex items-center justify-between mb-2">
        <div class="text-xs font-semibold text-muted-foreground uppercase">Explorer</div>
      </div>
      
      <div v-if="hostsStore.hosts.length === 0" class="text-sm text-muted-foreground italic p-2">
        No hosts saved.
      </div>

      <div class="space-y-1">
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

    <!-- Actions Footer -->
    <div class="border-t border-border pt-4 flex gap-2">
        <!-- Add Host Dialog -->
      <Dialog v-model:open="isAddModalOpen">
        <DialogTrigger as-child>
          <Button variant="outline" size="sm" class="flex-1 gap-1" title="Add Host" @click="resetForms">
            <Plus class="w-4 h-4" />
            Host
          </Button>
        </DialogTrigger>
        <DialogContent class="sm:max-w-[425px]">
          <DialogHeader>
            <DialogTitle>{{ editingHostId ? 'Edit Host' : 'Add SSH Host' }}</DialogTitle>
            <DialogDescription>
              Save host details securely. Passwords are encrypted locally.
            </DialogDescription>
          </DialogHeader>
          <div class="grid gap-4 py-4">
             <!-- Parent Folder Selection -->
             <div class="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="parent" class="text-right">Folder</Label>
              <select id="parent" v-model="newHost.parentId" class="col-span-3 flex h-10 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50">
                  <option :value="null">Root (None)</option>
                  <option v-for="f in formattedFolders" :key="f.id" :value="f.id">{{ f.displayName }}</option>
              </select>
            </div>

            <div class="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="name" class="text-right">Label</Label>
              <Input id="name" v-model="newHost.name" placeholder="Production Server" class="col-span-3" />
            </div>
            <div class="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="host" class="text-right">Host / IP</Label>
              <Input id="host" v-model="newHost.host" placeholder="192.168.1.1" class="col-span-3" />
            </div>
            <div class="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="port" class="text-right">Port</Label>
              <Input id="port" type="number" v-model="newHost.port" class="col-span-3" />
            </div>
            <div class="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="username" class="text-right">Username</Label>
              <Input id="username" v-model="newHost.username" placeholder="root" class="col-span-3" />
            </div>
            <div class="grid grid-cols-4 items-center gap-4">
              <Label htmlFor="password" class="text-right">Password</Label>
              <Input id="password" type="password" v-model="newHost.password" class="col-span-3" placeholder="Leave blank to keep unchanged" />
            </div>
          </div>
          <DialogFooter>
            <Button type="submit" @click="saveHost">{{ editingHostId ? 'Update' : 'Save' }}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <!-- New Folder Dialog -->
      <Dialog v-model:open="isFolderModalOpen">
        <DialogTrigger as-child>
            <Button variant="ghost" size="sm" class="px-2" title="New Folder" @click="resetForms">
             <FolderPlus class="w-4 h-4" />
          </Button>
        </DialogTrigger>
        <DialogContent class="sm:max-w-[425px]">
            <DialogHeader>
                <DialogTitle>New Folder</DialogTitle>
                <DialogDescription>Create a folder to organize your hosts.</DialogDescription>
            </DialogHeader>
            <div class="grid gap-4 py-4">
                 <div class="grid grid-cols-4 items-center gap-4">
                    <Label htmlFor="folderParent" class="text-right">Parent</Label>
                    <select id="folderParent" v-model="newFolder.parentId" class="col-span-3 flex h-10 w-full items-center justify-between rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50">
                        <option :value="null">Root (None)</option>
                         <option v-for="f in formattedFolders" :key="f.id" :value="f.id">{{ f.displayName }}</option>
                    </select>
                </div>
                <div class="grid grid-cols-4 items-center gap-4">
                    <Label htmlFor="folderName" class="text-right">Name</Label>
                    <Input id="folderName" v-model="newFolder.name" placeholder="My Project" class="col-span-3" />
                </div>
            </div>
            <DialogFooter>
                <Button type="submit" @click="saveFolder">Create Folder</Button>
            </DialogFooter>
        </DialogContent>
      </Dialog>
      
       <!-- Rename Folder Dialog -->
      <Dialog v-model:open="isRenameModalOpen">
        <DialogContent class="sm:max-w-[425px]">
            <DialogHeader>
                <DialogTitle>Rename Folder</DialogTitle>
            </DialogHeader>
            <div class="grid gap-4 py-4">
                <div class="grid grid-cols-4 items-center gap-4">
                    <Label htmlFor="renameName" class="text-right">Name</Label>
                    <Input id="renameName" v-model="renameData.name" class="col-span-3" @keyup.enter="saveRename" />
                </div>
            </div>
            <DialogFooter>
                <Button type="submit" @click="saveRename">Rename</Button>
            </DialogFooter>
        </DialogContent>
      </Dialog>
      
      <SettingsDialog v-model:open="isSettingsOpen" />
    </div>

    <!-- Settings Trigger -->
     <div class="border-t border-border pt-2 mt-2">
         <Button variant="ghost" class="w-full justify-start gap-2 text-muted-foreground hover:text-foreground" @click="isSettingsOpen = true">
            <Cog class="w-4 h-4" />
            Settings
         </Button>
    </div>

  </div>
</template>
