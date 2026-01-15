<script setup lang="ts">
import { ref } from 'vue'
import { useHostsStore } from '~/stores/hosts'
import { Plus, Monitor, Terminal as TerminalIcon } from 'lucide-vue-next'
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

const hostsStore = useHostsStore()
const isAddModalOpen = ref(false)

const newHost = ref({
    name: '',
    host: '',
    port: 22,
    username: '',
    password: ''
})

const emit = defineEmits(['connect'])

const saveHost = async () => {
    if (!newHost.value.name || !newHost.value.host || !newHost.value.username) return

    await hostsStore.addHost({
        name: newHost.value.name,
        host: newHost.value.host,
        port: newHost.value.port,
        username: newHost.value.username,
        password: newHost.value.password
    })

    // Reset form
    newHost.value = {
        name: '',
        host: '',
        port: 22,
        username: '',
        password: ''
    }
    isAddModalOpen.value = false
}

const connectToHost = async (hostId: string) => {
  const host = hostsStore.hosts.find(h => h.id === hostId)
  if (!host) return
  
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
        <div class="text-xs font-semibold text-muted-foreground uppercase">Saved Hosts</div>
      </div>
      
      <div v-if="hostsStore.hosts.length === 0" class="text-sm text-muted-foreground italic p-2">
        No hosts saved.
      </div>

      <div class="space-y-1">
        <div 
            v-for="h in hostsStore.hosts" 
            :key="h.id" 
            @click="connectToHost(h.id)"
            class="flex items-center gap-2 p-2 rounded-md hover:bg-muted cursor-pointer group transition-colors"
        >
            <Monitor class="w-4 h-4 text-muted-foreground group-hover:text-foreground" />
            <div class="flex flex-col overflow-hidden">
                <span class="text-sm font-medium truncate">{{ h.name }}</span>
                <span class="text-xs text-muted-foreground truncate">{{ h.username }}@{{ h.host }}</span>
            </div>
        </div>
      </div>
    </div>

    <div class="border-t border-border pt-4">
      <Dialog v-model:open="isAddModalOpen">
        <DialogTrigger as-child>
          <Button variant="outline" class="w-full gap-2">
            <Plus class="w-4 h-4" />
            Add New Host
          </Button>
        </DialogTrigger>
        <DialogContent class="sm:max-w-[425px]">
          <DialogHeader>
            <DialogTitle>Add SSH Host</DialogTitle>
            <DialogDescription>
              Save host details securely. Passwords are encrypted locally.
            </DialogDescription>
          </DialogHeader>
          <div class="grid gap-4 py-4">
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
              <Input id="password" type="password" v-model="newHost.password" class="col-span-3" />
            </div>
          </div>
          <DialogFooter>
            <Button type="submit" @click="saveHost">Save Host</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  </div>
</template>
