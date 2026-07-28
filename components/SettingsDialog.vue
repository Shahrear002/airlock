<script setup lang="ts">
import { ref } from 'vue'
import { useSettingsStore } from '~/stores/settings'
import { useHostsStore } from '~/stores/hosts'
import { save, open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { writeTextFile, readTextFile } from '@tauri-apps/plugin-fs'
import { exportData, importData } from '~/utils/backup'
import Dialog from '@/components/ui/dialog/Dialog.vue'
import DialogContent from '@/components/ui/dialog/DialogContent.vue'
import DialogDescription from '@/components/ui/dialog/DialogDescription.vue'
import DialogFooter from '@/components/ui/dialog/DialogFooter.vue'
import DialogHeader from '@/components/ui/dialog/DialogHeader.vue'
import DialogTitle from '@/components/ui/dialog/DialogTitle.vue'
import Button from '@/components/ui/button/Button.vue'
import Input from '@/components/ui/input/Input.vue'
import Label from '@/components/ui/label/Label.vue'
import { Shield, Upload, Download, Loader2, Terminal as TerminalIcon, Plus } from 'lucide-vue-next'

// ... existing imports ...

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits(['update:open'])
const hostsStore = useHostsStore()
const settingsStore = useSettingsStore()

const handleImportTheme = async () => {
    try {
        const selected = await openFileDialog({
            multiple: false,
            filters: [{
                name: 'Theme JSON',
                extensions: ['json']
            }]
        })
        
        if (selected) {
             const content = await readTextFile(selected as string)
             const theme = JSON.parse(content)
             // Simple validation
             if (!theme.background || !theme.foreground) {
                 // You might want to show an error toast here
                 console.error("Invalid theme file")
                 return
             }
             
             // Use filename as theme name
             const pathString = selected as string
             // Handle both windows and unix separators
             const fileName = pathString.split(/[\\/]/).pop()?.replace('.json', '') || 'Custom Theme'
             
             await settingsStore.importTheme(fileName, theme)
        }
    } catch (e) {
        console.error('Failed to import theme:', e)
    }
}

const exportPassword = ref('')
const importPassword = ref('')
const isExporting = ref(false)
const isImporting = ref(false)
const importError = ref('')
const importSuccess = ref('')
const fileInput = ref<HTMLInputElement | null>(null)

const handleExport = async () => {
    if (!exportPassword.value) return
    isExporting.value = true
    
    try {
        const dataToExport = await hostsStore.exportState()
        const encryptedData = await exportData(dataToExport, exportPassword.value)
        
        // Use Tauri Save Dialog to let user pick location
        const filePath = await save({
            filters: [{
                name: 'Airlock Backup',
                extensions: ['json']
            }],
            defaultPath: `airlock-backup-${new Date().toISOString().split('T')[0]}.json`
        })

        if (!filePath) {
            isExporting.value = false
            return // User cancelled
        }

        await writeTextFile(filePath, encryptedData)
        
        exportPassword.value = ''
        // Show success feedback? We can use the importSuccess var for now or a new one
        importSuccess.value = `Backup saved to: ${filePath}`
        setTimeout(() => {
            importSuccess.value = ''
        }, 5000)

    } catch (e: any) {
        console.error(e)
        importError.value = `Failed to save`
        // importError.value = `Failed to save: ${e.message || e}`
    } finally {
        isExporting.value = false
    }
}

const triggerImport = () => {
    fileInput.value?.click()
}

const onFileSelected = async (event: Event) => {
    const file = (event.target as HTMLInputElement).files?.[0]
    if (!file) return

    isImporting.value = true
    importError.value = ''
    importSuccess.value = ''

    try {
        const text = await file.text()
        // We need password first.
        // Actually, flow: Select File -> Prompt Password -> Decrypt
        // So we store text temporarily?
        // Let's change flow: Keep file in memory, wait for user to enter password and click "Restore"
        selectedFileContent.value = text
    } catch (e) {
        importError.value = "Failed to read file"
    } finally {
        isImporting.value = false
    }
}

const selectedFileContent = ref<string | null>(null)

const handleImport = async () => {
    if (!selectedFileContent.value || !importPassword.value) return
    
    isImporting.value = true
    importError.value = ''
    
    try {
        const data = await importData(selectedFileContent.value, importPassword.value)
        await hostsStore.importState(data)
        importSuccess.value = "Backup restored successfully!"
        selectedFileContent.value = null
        importPassword.value = ''
        setTimeout(() => {
            importSuccess.value = ''
            emit('update:open', false)
        }, 1500)
    } catch (e) {
        importError.value = "Invalid password or corrupted file."
    } finally {
        isImporting.value = false
    }
}

const reset = () => {
    selectedFileContent.value = null
    importPassword.value = ''
    exportPassword.value = ''
    importError.value = ''
    importSuccess.value = ''
}
</script>

<template>
  <Dialog :open="open" @update:open="$emit('update:open', $event)">
    <DialogContent 
        class="sm:max-w-[500px] resize overflow-y-auto overflow-x-hidden min-h-[400px] max-h-[85vh] flex flex-col"
        :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-[#E9EDF1] shadow-[0_0_80px_rgba(0,0,0,0.8)]' : ''"
    >
      <DialogHeader>
        <DialogTitle :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]' : ''">Settings & Data</DialogTitle>
        <DialogDescription :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/60' : ''">
          Manage your Airlock options and backups.
        </DialogDescription>
      </DialogHeader>
      
      <div class="space-y-6 py-4 flex-1">
        
        <!-- Appearance Section -->
         <div class="space-y-4 border rounded-lg p-4 transition-colors" :class="settingsStore.appTheme === 'glass' ? 'bg-white/5 border-white/10' : 'bg-muted/20 border-border'">
            <div class="flex items-center gap-2 font-semibold">
                <TerminalIcon class="w-4 h-4" /> Appearance
            </div>
           <div class="grid grid-cols-4 items-center gap-4 mb-3">
               <Label class="text-right" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/80' : ''">App Theme</Label>
               <div class="col-span-3">
                   <select 
                      v-model="settingsStore.appTheme" 
                      class="flex h-10 w-full items-center justify-between rounded-md border px-3 py-2 text-sm disabled:cursor-not-allowed disabled:opacity-50"
                      :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-white focus:!outline-none focus:!ring-2 focus:!ring-[#2D8CFF]/40 !ring-offset-0' : 'bg-background border-input text-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 ring-offset-background placeholder:text-muted-foreground'"
                    >
                       <option value="default">Default</option>
                       <option value="glass">Glassmorphism</option>
                   </select>
               </div>
            </div>
            <div class="grid grid-cols-4 items-center gap-4">
               <Label class="text-right" :class="settingsStore.appTheme === 'glass' ? '!text-[#E9EDF1]/80' : ''">Terminal</Label>
               <div class="col-span-3 flex gap-2">
                   <select 
                      v-model="settingsStore.terminalThemeName" 
                      class="flex-1 flex h-10 w-full items-center justify-between rounded-md border px-3 py-2 text-sm disabled:cursor-not-allowed disabled:opacity-50"
                      :class="settingsStore.appTheme === 'glass' ? '!bg-[#0A0D14] !border-white/10 !text-white focus:!outline-none focus:!ring-2 focus:!ring-[#2D8CFF]/40 !ring-offset-0' : 'bg-background border-input text-foreground focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 ring-offset-background placeholder:text-muted-foreground'"
                      @change="settingsStore.setTerminalTheme(($event.target as HTMLSelectElement).value)"
                    >
                       <option v-for="(theme, name) in settingsStore.allThemes" :key="name" :value="name">
                           {{ name }}
                       </option>
                   </select>
                   <Button variant="outline" size="icon" @click="handleImportTheme" title="Import Theme (JSON)" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">
                       <Plus class="w-4 h-4" />
                   </Button>
               </div>
            </div>
         </div>

        <!-- Export Section -->

        <div class="space-y-4 border rounded-lg p-4 transition-colors" :class="settingsStore.appTheme === 'glass' ? 'bg-white/5 border-white/10' : 'bg-muted/20 border-border'">
            <div class="flex items-center gap-2 font-semibold">
                <Upload class="w-4 h-4" /> Export Backup
            </div>
            <p class="text-sm" :class="settingsStore.appTheme === 'glass' ? 'text-white/60' : 'text-muted-foreground'">
                Create an encrypted backup of all your hosts and folders.
            </p>
            <div class="flex gap-2">
                <Input type="password" v-model="exportPassword" placeholder="Set a backup password" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                <Button @click="handleExport" :disabled="!exportPassword || isExporting" :class="settingsStore.appTheme === 'glass' ? 'btn-primary' : ''">
                    <Loader2 v-if="isExporting" class="w-4 h-4 animate-spin mr-2" />
                    Download
                </Button>
            </div>
        </div>

        <!-- Import Section -->
        <div class="space-y-4 border rounded-lg p-4 transition-colors" :class="settingsStore.appTheme === 'glass' ? 'bg-white/5 border-white/10' : 'bg-muted/20 border-border'">
             <div class="flex items-center gap-2 font-semibold">
                <Download class="w-4 h-4" /> Import Backup
            </div>
             <p class="text-sm" :class="settingsStore.appTheme === 'glass' ? 'text-white/60' : 'text-muted-foreground'">
                Restore from an existing backup file. Warning: This will overwrite current data.
            </p>

            <div v-if="!selectedFileContent">
                <input type="file" ref="fileInput" class="hidden" accept=".json" @change="onFileSelected" />
                <Button variant="outline" class="w-full" @click="triggerImport" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">
                    Select Backup File
                </Button>
            </div>

            <div v-else class="space-y-3 animate-in fade-in">
                 <div class="text-xs font-mono p-2 rounded" :class="settingsStore.appTheme === 'glass' ? 'bg-white/10 text-white' : 'bg-muted'">
                    File loaded. Enter password to decrypt.
                 </div>
                 <div class="flex gap-2">
                    <Input type="password" v-model="importPassword" placeholder="Enter backup password" :class="settingsStore.appTheme === 'glass' ? '!bg-white/5 !border-white/10 !text-white placeholder:text-white/30 focus-visible:!ring-[#2D8CFF]/40' : ''" />
                    <Button @click="handleImport" variant="destructive" :disabled="!importPassword || isImporting" :class="settingsStore.appTheme === 'glass' ? '!bg-red-500/20 hover:!bg-red-500/30 !text-red-400 !border-red-500/30' : ''">
                        <Loader2 v-if="isImporting" class="w-4 h-4 animate-spin mr-2" />
                        Restore
                    </Button>
                </div>
                 <Button variant="outline" size="sm" class="w-full" @click="reset" :class="settingsStore.appTheme === 'glass' ? '!bg-transparent !border-white/10 hover:!bg-white/10 !text-white' : ''">Cancel</Button>
            </div>

            <div v-if="importError" class="text-xs font-medium p-2 rounded" :class="settingsStore.appTheme === 'glass' ? 'bg-red-500/20 text-red-400' : 'text-destructive bg-destructive/10'">
                {{ importError }}
            </div>
            <div v-if="importSuccess" class="text-xs font-medium p-2 rounded" :class="settingsStore.appTheme === 'glass' ? 'bg-[#1DE9B6]/20 text-[#1DE9B6]' : 'text-green-500 bg-green-500/10'">
                 {{ importSuccess }}
            </div>
        </div>

      </div>
    </DialogContent>
  </Dialog>
</template>
