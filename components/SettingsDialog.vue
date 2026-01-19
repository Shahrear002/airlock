<script setup lang="ts">
import { ref } from 'vue'
import { useHostsStore } from '~/stores/hosts'
import { save } from '@tauri-apps/plugin-dialog'
import { writeTextFile } from '@tauri-apps/plugin-fs'
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
import { Shield, Upload, Download, Loader2 } from 'lucide-vue-next'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits(['update:open'])
const hostsStore = useHostsStore()

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
    <DialogContent class="sm:max-w-[500px]">
      <DialogHeader>
        <DialogTitle>Settings & Data</DialogTitle>
        <DialogDescription>
          Manage your Airlock options and backups.
        </DialogDescription>
      </DialogHeader>
      
      <div class="space-y-6 py-4">
        
        <!-- Export Section -->
        <div class="space-y-4 border rounded-lg p-4 bg-muted/20">
            <div class="flex items-center gap-2 font-semibold">
                <Upload class="w-4 h-4" /> Export Backup
            </div>
            <p class="text-sm text-muted-foreground">
                Create an encrypted backup of all your hosts and folders.
            </p>
            <div class="flex gap-2">
                <Input type="password" v-model="exportPassword" placeholder="Set a backup password" />
                <Button @click="handleExport" :disabled="!exportPassword || isExporting">
                    <Loader2 v-if="isExporting" class="w-4 h-4 animate-spin mr-2" />
                    Download
                </Button>
            </div>
        </div>

        <!-- Import Section -->
        <div class="space-y-4 border rounded-lg p-4 bg-muted/20">
             <div class="flex items-center gap-2 font-semibold">
                <Download class="w-4 h-4" /> Import Backup
            </div>
             <p class="text-sm text-muted-foreground">
                Restore from an existing backup file. Warning: This will overwrite current data.
            </p>

            <div v-if="!selectedFileContent">
                <input type="file" ref="fileInput" class="hidden" accept=".json" @change="onFileSelected" />
                <Button variant="outline" class="w-full" @click="triggerImport">
                    Select Backup File
                </Button>
            </div>

            <div v-else class="space-y-3 animate-in fade-in">
                 <div class="text-xs font-mono bg-muted p-2 rounded">
                    File loaded. Enter password to decrypt.
                 </div>
                 <div class="flex gap-2">
                    <Input type="password" v-model="importPassword" placeholder="Enter backup password" />
                    <Button @click="handleImport" variant="destructive" :disabled="!importPassword || isImporting">
                        <Loader2 v-if="isImporting" class="w-4 h-4 animate-spin mr-2" />
                        Restore
                    </Button>
                </div>
                 <Button variant="ghost" size="sm" class="w-full" @click="reset">Cancel</Button>
            </div>

            <div v-if="importError" class="text-xs text-destructive font-medium bg-destructive/10 p-2 rounded">
                {{ importError }}
            </div>
            <div v-if="importSuccess" class="text-xs text-green-500 font-medium bg-green-500/10 p-2 rounded">
                 {{ importSuccess }}
            </div>
        </div>

      </div>
    </DialogContent>
  </Dialog>
</template>
