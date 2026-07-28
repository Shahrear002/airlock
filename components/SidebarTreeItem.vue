<script setup lang="ts">
import { ref, computed } from 'vue'
import { type Host, useHostsStore } from '~/stores/hosts'
import { useSettingsStore } from '~/stores/settings'
import { ChevronRight, ChevronDown, Folder, Monitor, Trash2, FolderOpen } from 'lucide-vue-next'
import { ask } from '@tauri-apps/plugin-dialog'
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'

const props = defineProps<{
  item: Host
  depth: number
}>()

const emit = defineEmits(['connect', 'select-folder', 'edit', 'rename', 'create-host', 'create-folder'])
const hostsStore = useHostsStore()
const settingsStore = useSettingsStore()
const isOpen = ref(false)

const children = computed(() => hostsStore.getChildren(props.item.id))
const hasChildren = computed(() => children.value.length > 0)

const toggleOpen = () => {
    if (props.item.type === 'folder') {
        isOpen.value = !isOpen.value
    }
}

const handleClick = () => {
    if (props.item.type === 'folder') {
        toggleOpen()
    } else {
        emit('connect', props.item.id)
    }
}

const handleDelete = async () => {
    const yes = await ask(`Are you sure you want to delete ${props.item.name}?`, {
        title: 'Delete Confirmation',
        kind: 'warning'
    });
    
    if (yes) {
        hostsStore.removeItem(props.item.id)
    }
}
</script>

<template>
  <div>
    <ContextMenu>
        <ContextMenuTrigger as-child>
            <div 
                class="flex items-center gap-1 p-1 rounded-md cursor-pointer group transition-colors select-none text-sm"
                :class="settingsStore.appTheme === 'glass' ? 'hover:bg-white/5 text-[#E9EDF1]' : 'hover:bg-muted'"
                :style="{ paddingLeft: `${props.depth * 24 + 12}px` }"
                @click="handleClick"
            >
                <!-- Folder Icon / Chevron -->
                <div v-if="props.item.type === 'folder'" class="flex items-center mr-1" :class="settingsStore.appTheme === 'glass' ? 'text-white/60' : 'text-muted-foreground'">
                    <component :is="isOpen ? ChevronDown : ChevronRight" class="w-3 h-3" />
                    <component :is="isOpen ? FolderOpen : Folder" class="w-4 h-4 ml-1" />
                </div>
                
                <!-- Host Icon -->
                <Monitor v-else class="w-4 h-4 mr-2" :class="settingsStore.appTheme === 'glass' ? 'text-white/60 group-hover:text-white' : 'text-muted-foreground group-hover:text-foreground'" />

                <!-- Name -->
                <span class="flex-1 truncate font-medium" :class="settingsStore.appTheme === 'glass' ? 'text-white/90 group-hover:text-white' : 'text-foreground/80 group-hover:text-foreground'">
                    {{ props.item.name }}
                </span>
            </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
            <template v-if="props.item.type === 'folder'">
                <ContextMenuItem @select="$emit('rename', props.item)">Rename</ContextMenuItem>
                <ContextMenuItem @select="$emit('create-host', props.item.id)">Add Host Here</ContextMenuItem>
                <ContextMenuItem @select="$emit('create-folder', props.item.id)">Add Folder Here</ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem class="text-destructive focus:text-destructive" @select="handleDelete">Delete</ContextMenuItem>
            </template>
            <template v-else>
                <ContextMenuItem @select="$emit('connect', props.item.id)">Connect</ContextMenuItem>
                <ContextMenuItem @select="$emit('edit', props.item)">Edit</ContextMenuItem>
                <ContextMenuSeparator />
                <ContextMenuItem class="text-destructive focus:text-destructive" @select="handleDelete">Delete</ContextMenuItem>
            </template>
        </ContextMenuContent>
    </ContextMenu>

    <!-- Children (Recursive) -->
    <div v-if="props.item.type === 'folder' && isOpen">
        <SidebarTreeItem 
            v-for="child in children" 
            :key="child.id" 
            :item="child" 
            :depth="props.depth + 1"
            @connect="$emit('connect', $event)"
            @edit="$emit('edit', $event)"
            @rename="$emit('rename', $event)"
            @create-host="$emit('create-host', $event)"
            @create-folder="$emit('create-folder', $event)"
        />
        <div v-if="children.length === 0" class="text-xs text-muted-foreground italic py-1" :style="{ paddingLeft: `${(props.depth + 1) * 24 + 20}px` }">
            Empty
        </div>
    </div>
  </div>
</template>
