<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { FileCode, Folder, RefreshCw, X, Server, HardDrive, Loader2 } from 'lucide-vue-next'
import { useTabsStore } from '~/stores/tabs'

const props = defineProps<{
    sessionId: string,
    connectionId?: string
}>()

const tabsStore = useTabsStore()

interface FileEntry {
    name: string
    is_dir: boolean
    size: number
    mtime: number
}

// Remote State
const remotePath = ref('/')
const remoteFiles = ref<FileEntry[]>([])
const remoteLoading = ref(false)
const remoteError = ref('')

// Local State
const localPath = ref('')
const localFiles = ref<FileEntry[]>([])
const localLoading = ref(false)
const localError = ref('')

// Transfer State
const transferLoading = ref(false)
const transferProgress = ref('')

// Fake Drag & Drop State
const draggedItem = ref<{ type: 'local'|'remote', name: string, is_dir: boolean } | null>(null)
const mousePos = ref({ x: 0, y: 0 })
const hoverTarget = ref<'local'|'remote'|null>(null)

const joinPath = (base: string, name: string) => {
    if (base.endsWith('/') || base.endsWith('\\')) {
        return base + name;
    }
    const sep = base.includes('\\') ? '\\' : '/'
    return base + sep + name;
}

const loadRemoteDir = async (path: string) => {
    remoteLoading.value = true
    remoteError.value = ''
    try {
        const backendId = props.connectionId || props.sessionId
        const res = await invoke<FileEntry[]>('sftp_list_dir', { id: backendId, path })
        remoteFiles.value = res
        remotePath.value = path
    } catch (e: any) {
        remoteError.value = e.toString()
    } finally {
        remoteLoading.value = false
    }
}

const loadLocalDir = async (path: string) => {
    localLoading.value = true
    localError.value = ''
    try {
        const res = await invoke<FileEntry[]>('local_list_dir', { path })
        localFiles.value = res
        localPath.value = path
    } catch (e: any) {
        localError.value = e.toString()
    } finally {
        localLoading.value = false
    }
}

onMounted(async () => {
    // Load local home dir
    try {
        const home = await invoke<string>('get_local_home_dir')
        localPath.value = home
        loadLocalDir(home)
    } catch (e) {
        localPath.value = 'C:\\'
        loadLocalDir('C:\\')
    }

    setTimeout(() => {
        loadRemoteDir(remotePath.value)
    }, 1000)
})

const navigateRemote = (name: string) => {
    if (name === '..') {
        const parts = remotePath.value.split('/').filter(Boolean)
        parts.pop()
        loadRemoteDir('/' + (parts.length > 0 ? parts.join('/') : ''))
    } else {
        loadRemoteDir(joinPath(remotePath.value, name))
    }
}

const navigateLocal = (name: string) => {
    if (name === '..') {
        const isWindows = localPath.value.includes('\\')
        const sep = isWindows ? '\\' : '/'
        const parts = localPath.value.split(sep).filter(Boolean)
        
        if (parts.length > 1) {
             parts.pop()
             const newPath = parts.join(sep) + (isWindows && parts.length === 1 ? sep : '')
             loadLocalDir(newPath)
        } else if (parts.length === 1 && isWindows) {
             loadLocalDir(parts[0] + sep)
        } else {
             loadLocalDir('/')
        }
    } else {
        loadLocalDir(joinPath(localPath.value, name))
    }
}

const formatSize = (bytes: number) => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}

const closePane = () => {
    tabsStore.removePane(props.sessionId)
}

const activatePane = () => {
    tabsStore.setActivePane(props.sessionId)
}

// Custom Drag and Drop Handlers
const onMouseDown = (event: MouseEvent, type: 'local'|'remote', name: string, is_dir: boolean) => {
    if (event.button !== 0) return // Only process left click
    
    draggedItem.value = { type, name, is_dir }
    mousePos.value = { x: event.clientX, y: event.clientY }
    
    // Bind global listeners to track drag anywhere on screen
    window.addEventListener('mousemove', onMouseMove, { capture: true })
    window.addEventListener('mouseup', onMouseUp, { capture: true })
    
    // Prevent default text selection during drag
    event.preventDefault()
}

const onMouseMove = (e: MouseEvent) => {
    if (!draggedItem.value) return
    mousePos.value = { x: e.clientX, y: e.clientY }
    
    // Determine which pane we are hovering over
    const elements = document.elementsFromPoint(e.clientX, e.clientY)
    let target: 'local'|'remote'|null = null
    for (const el of elements) {
        if (el.id === 'pane-local') target = 'local'
        if (el.id === 'pane-remote') target = 'remote'
    }
    hoverTarget.value = target
}

const onMouseUp = (e: MouseEvent) => {
    window.removeEventListener('mousemove', onMouseMove, { capture: true })
    window.removeEventListener('mouseup', onMouseUp, { capture: true })
    
    const item = draggedItem.value
    const target = hoverTarget.value
    
    draggedItem.value = null
    hoverTarget.value = null
    
    // If dropped in a different pane, execute transfer
    if (item && target && item.type !== target) {
        if (target === 'local') {
            doDropLocal(item)
        } else if (target === 'remote') {
            doDropRemote(item)
        }
    }
}

const doDropLocal = async (item: {name: string, type: string}) => {
    transferLoading.value = true
    transferProgress.value = `Downloading ${item.name}...`
    try {
        const backendId = props.connectionId || props.sessionId
        const rPath = joinPath(remotePath.value, item.name)
        const lPath = joinPath(localPath.value, item.name)
        
        await invoke('sftp_download', { id: backendId, remotePath: rPath, localPath: lPath })
        loadLocalDir(localPath.value)
    } catch (e: any) {
        alert("Download failed: " + e.toString())
    } finally {
        transferLoading.value = false
    }
}

const doDropRemote = async (item: {name: string, type: string}) => {
    transferLoading.value = true
    transferProgress.value = `Uploading ${item.name}...`
    try {
        const backendId = props.connectionId || props.sessionId
        const lPath = joinPath(localPath.value, item.name)
        const rPath = joinPath(remotePath.value, item.name)
        
        await invoke('sftp_upload', { id: backendId, localPath: lPath, remotePath: rPath })
        loadRemoteDir(remotePath.value)
    } catch (e: any) {
        alert("Upload failed: " + e.toString())
    } finally {
        transferLoading.value = false
    }
}
</script>

<template>
  <div class="h-full w-full bg-background flex flex-col overflow-hidden text-sm relative group border-l border-border/50" @click.stop="activatePane">
      
      <!-- Custom Drag Ghost -->
      <Teleport to="body">
          <div v-if="draggedItem" 
               class="fixed pointer-events-none z-[9999] backdrop-blur-md bg-[#1e1e2e]/90 border border-white/10 px-4 py-2.5 rounded-xl shadow-[0_10px_40px_rgba(0,0,0,0.5)] flex items-center gap-3 opacity-100 text-sm font-medium transition-transform duration-75"
               :style="{ left: mousePos.x + 20 + 'px', top: mousePos.y + 20 + 'px', transform: 'rotate(-3deg)' }">
              <Folder v-if="draggedItem.is_dir" class="w-5 h-5 text-blue-400" />
              <FileCode v-else class="w-5 h-5 text-indigo-400" />
              <span class="truncate max-w-[250px] text-gray-200">{{ draggedItem.name }}</span>
          </div>
      </Teleport>

      <!-- Transfer Notification Toast -->
      <div v-if="transferLoading" 
           class="absolute bottom-6 right-6 z-50 bg-[#1e1e2e]/95 backdrop-blur-xl border border-white/10 shadow-[0_8px_30px_rgba(0,0,0,0.5)] pl-5 pr-2 py-3 rounded-2xl flex items-center gap-4 text-sm transform transition-all animate-in slide-in-from-bottom-5 fade-in">
          <div class="h-10 w-10 rounded-full bg-blue-500/10 flex items-center justify-center border border-blue-500/20 shrink-0">
              <Loader2 class="w-5 h-5 animate-spin text-blue-400" />
          </div>
          <div class="flex flex-col min-w-[150px] max-w-[250px]">
              <span class="text-gray-200 font-medium tracking-tight">Transferring File</span>
              <span class="text-gray-400 text-xs truncate">{{ transferProgress }}</span>
          </div>
          <button @click="transferLoading = false" class="p-1.5 hover:bg-white/10 rounded-full text-gray-500 hover:text-gray-300 transition-colors ml-1" title="Dismiss notification">
              <X class="w-4 h-4" />
          </button>
      </div>

      <!-- Top Title Bar -->
      <div class="flex items-center justify-between p-2.5 bg-[#181825] border-b border-white/5 z-20 shadow-sm relative">
          <div class="font-semibold text-xs text-gray-400 uppercase tracking-wider flex items-center gap-2">
            SFTP File Transfer
          </div>
          <button @click="closePane" class="p-1.5 hover:bg-white/5 hover:text-white rounded-md text-gray-500 transition-colors">
              <X class="w-4 h-4" />
          </button>
      </div>

      <!-- Dual Pane Grid -->
      <div class="flex-1 grid grid-cols-2 divide-x divide-border/50 overflow-hidden relative">
          
          <!-- Local Pane (Left) -->
          <div id="pane-local" class="flex flex-col overflow-hidden bg-[#1e1e2e] relative"
               :class="{'ring-2 ring-blue-500/50 shadow-[inset_0_0_50px_rgba(59,130,246,0.1)] z-10': hoverTarget === 'local' && draggedItem?.type === 'remote'}"
          >
              <!-- Local Header -->
              <div class="flex items-center gap-3 p-3 bg-gradient-to-b from-[#2a2a3c] to-[#1e1e2e] border-b border-white/5 shadow-sm z-20 pointer-events-auto">
                  <HardDrive class="w-4 h-4 text-indigo-400" />
                  <span class="font-medium text-xs text-gray-200 tracking-wide">Local</span>
                  <input type="text" 
                         :value="localPath"
                         @keydown.enter="loadLocalDir(($event.target as HTMLInputElement).value)"
                         @blur="($event.target as HTMLInputElement).value = localPath"
                         class="flex-1 font-mono text-xs px-3 py-1.5 bg-black/20 rounded-lg border border-white/5 focus:border-indigo-500/50 focus:bg-black/40 focus:outline-none transition-colors text-gray-300" />
                  <button @click="loadLocalDir(localPath)" class="p-1.5 hover:bg-white/10 rounded-lg text-gray-400 transition-colors">
                      <RefreshCw class="w-4 h-4" :class="{'animate-spin text-indigo-400': localLoading}" />
                  </button>
              </div>
              
              <!-- Local Files -->
              <div class="flex-1 overflow-auto relative pointer-events-auto">
                  <div v-if="localError" class="text-red-400 p-4 text-xs bg-red-400/10 rounded-lg m-3 border border-red-400/20">{{ localError }}</div>
                  
                  <!-- Drop Zone Overlay -->
                  <div class="absolute inset-0 z-30 pointer-events-none transition-all duration-300 flex items-center justify-center"
                       :class="(draggedItem?.type === 'remote') ? (hoverTarget === 'local' ? 'bg-blue-500/10 backdrop-blur-[2px] opacity-100' : 'bg-transparent opacity-0') : 'opacity-0'">
                      <div class="bg-blue-600/90 backdrop-blur-md text-white px-6 py-3.5 rounded-2xl font-medium shadow-[0_10px_40px_rgba(37,99,235,0.4)] flex items-center gap-3 transform transition-all duration-300 border border-blue-400/30"
                           :class="hoverTarget === 'local' ? 'scale-100 translate-y-0' : 'scale-90 translate-y-4'">
                          <HardDrive class="w-5 h-5 text-blue-200" />
                          Drop to Download Here
                      </div>
                  </div>

                  <table class="w-full text-left border-collapse select-none" v-if="!localError" :class="{'opacity-40 grayscale-[30%]': draggedItem}">
                      <tbody>
                          <tr v-if="localPath && localPath !== '/' && !localPath.endsWith(':\\')" @dblclick.stop="navigateLocal('..')" class="hover:bg-white/[0.03] cursor-pointer transition-colors duration-150">
                              <td class="p-2.5 px-4 flex items-center gap-3 text-gray-400">
                                  <Folder class="w-4 h-4" /> <span class="font-medium">..</span>
                              </td>
                              <td class="p-2.5 px-4 text-gray-500 text-right">-</td>
                          </tr>
                          <tr v-for="file in localFiles" :key="file.name" 
                              @dblclick.stop="file.is_dir ? navigateLocal(file.name) : null" 
                              @mousedown="onMouseDown($event, 'local', file.name, file.is_dir)"
                              class="hover:bg-white/[0.03] cursor-pointer border-b border-white/[0.02] group/row active:bg-blue-500/10 transition-colors duration-150"
                              :class="{'bg-blue-500/20': draggedItem?.name === file.name && draggedItem?.type === 'local'}">
                              <td class="p-2.5 px-4 flex items-center gap-3 truncate">
                                  <Folder v-if="file.is_dir" class="w-4 h-4 text-indigo-400 group-hover/row:text-indigo-300 shrink-0 transition-colors" />
                                  <FileCode v-else class="w-4 h-4 text-gray-500 group-hover/row:text-gray-400 shrink-0 transition-colors" />
                                  <span class="truncate font-medium" :class="{'text-gray-300 group-hover/row:text-gray-200': !file.is_dir, 'text-indigo-200 group-hover/row:text-indigo-100': file.is_dir}">{{ file.name }}</span>
                              </td>
                              <td class="p-2.5 px-4 text-gray-500 whitespace-nowrap text-xs text-right tracking-wider">
                                  {{ file.is_dir ? '-' : formatSize(file.size) }}
                              </td>
                          </tr>
                      </tbody>
                  </table>
              </div>
          </div>

          <!-- Remote Pane (Right) -->
          <div id="pane-remote" class="flex flex-col overflow-hidden bg-[#1e1e2e] relative"
               :class="{'ring-2 ring-blue-500/50 shadow-[inset_0_0_50px_rgba(59,130,246,0.1)] z-10': hoverTarget === 'remote' && draggedItem?.type === 'local'}"
          >
              <!-- Remote Header -->
              <div class="flex items-center gap-3 p-3 bg-gradient-to-b from-[#2a2a3c] to-[#1e1e2e] border-b border-white/5 shadow-sm z-20 pointer-events-auto">
                  <Server class="w-4 h-4 text-emerald-400" />
                  <span class="font-medium text-xs text-gray-200 tracking-wide">Remote</span>
                  <input type="text" 
                         :value="remotePath"
                         @keydown.enter="loadRemoteDir(($event.target as HTMLInputElement).value)"
                         @blur="($event.target as HTMLInputElement).value = remotePath"
                         class="flex-1 font-mono text-xs px-3 py-1.5 bg-black/20 rounded-lg border border-white/5 focus:border-emerald-500/50 focus:bg-black/40 focus:outline-none transition-colors text-gray-300" />
                  <button @click="loadRemoteDir(remotePath)" class="p-1.5 hover:bg-white/10 rounded-lg text-gray-400 transition-colors">
                      <RefreshCw class="w-4 h-4" :class="{'animate-spin text-emerald-400': remoteLoading}" />
                  </button>
              </div>
              
              <!-- Remote Files -->
              <div class="flex-1 overflow-auto relative pointer-events-auto">
                  <div v-if="remoteError" class="text-red-400 p-4 text-xs bg-red-400/10 rounded-lg m-3 border border-red-400/20">{{ remoteError }}</div>

                  <!-- Drop Zone Overlay -->
                  <div class="absolute inset-0 z-30 pointer-events-none transition-all duration-300 flex items-center justify-center"
                       :class="(draggedItem?.type === 'local') ? (hoverTarget === 'remote' ? 'bg-emerald-500/10 backdrop-blur-[2px] opacity-100' : 'bg-transparent opacity-0') : 'opacity-0'">
                      <div class="bg-emerald-600/90 backdrop-blur-md text-white px-6 py-3.5 rounded-2xl font-medium shadow-[0_10px_40px_rgba(16,185,129,0.4)] flex items-center gap-3 transform transition-all duration-300 border border-emerald-400/30"
                           :class="hoverTarget === 'remote' ? 'scale-100 translate-y-0' : 'scale-90 translate-y-4'">
                          <Server class="w-5 h-5 text-emerald-200" />
                          Drop to Upload Here
                      </div>
                  </div>

                  <table class="w-full text-left border-collapse select-none" v-if="!remoteError" :class="{'opacity-40 grayscale-[30%]': draggedItem}">
                      <tbody>
                          <tr v-if="remotePath !== '/'" @dblclick.stop="navigateRemote('..')" class="hover:bg-white/[0.03] cursor-pointer transition-colors duration-150">
                              <td class="p-2.5 px-4 flex items-center gap-3 text-gray-400">
                                  <Folder class="w-4 h-4" /> <span class="font-medium">..</span>
                              </td>
                              <td class="p-2.5 px-4 text-gray-500 text-right">-</td>
                          </tr>
                          <tr v-for="file in remoteFiles" :key="file.name" 
                              @dblclick.stop="file.is_dir ? navigateRemote(file.name) : null" 
                              @mousedown="onMouseDown($event, 'remote', file.name, file.is_dir)"
                              class="hover:bg-white/[0.03] cursor-pointer border-b border-white/[0.02] group/row active:bg-blue-500/10 transition-colors duration-150"
                              :class="{'bg-blue-500/20': draggedItem?.name === file.name && draggedItem?.type === 'remote'}">
                              <td class="p-2.5 px-4 flex items-center gap-3 truncate">
                                  <Folder v-if="file.is_dir" class="w-4 h-4 text-emerald-400 group-hover/row:text-emerald-300 shrink-0 transition-colors" />
                                  <FileCode v-else class="w-4 h-4 text-gray-500 group-hover/row:text-gray-400 shrink-0 transition-colors" />
                                  <span class="truncate font-medium" :class="{'text-gray-300 group-hover/row:text-gray-200': !file.is_dir, 'text-emerald-200 group-hover/row:text-emerald-100': file.is_dir}">{{ file.name }}</span>
                              </td>
                              <td class="p-2.5 px-4 text-gray-500 whitespace-nowrap text-xs text-right tracking-wider">
                                  {{ file.is_dir ? '-' : formatSize(file.size) }}
                              </td>
                          </tr>
                      </tbody>
                  </table>
              </div>
          </div>

      </div>
  </div>
</template>
