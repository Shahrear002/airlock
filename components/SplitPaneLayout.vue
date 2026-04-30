<script setup lang="ts">
import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'
import { type LayoutNode, useTabsStore } from '~/stores/tabs'
import Terminal from './Terminal.vue'
import SftpBrowser from './SftpBrowser.vue'
import { computed } from 'vue'

const props = defineProps<{
    node: LayoutNode
}>()

const tabsStore = useTabsStore()

// Computed property to handle children mutability if necessary
const children = computed(() => props.node.children || [])

const onResize = (event: any) => {
    // We could update store with new sizes here if we want persistence
    // event is array of { min, max, size, ... }
}
</script>

<template>
  <div class="h-full w-full">
      <!-- Leaf Node: Render Terminal or SFTP -->
      <div v-if="node.type === 'leaf'" class="h-full w-full relative group">
          <Terminal v-if="!node.paneType || node.paneType === 'terminal'" :session-id="node.sessionId!" :connection-id="node.connectionId" />
          <SftpBrowser v-else-if="node.paneType === 'sftp'" :session-id="node.sessionId!" :connection-id="node.connectionId" />
          
          <!-- Overlay to indicate focus (optional) -->
          <div 
            v-if="tabsStore.activePaneId === node.sessionId" 
            class="absolute inset-0 pointer-events-none border-2 border-primary opacity-20"
          ></div>
      </div>

      <!-- Split Node: Render Splitpanes -->
      <Splitpanes 
        v-else 
        class="default-theme" 
        :horizontal="node.type === 'horizontal'"
        @resize="onResize"
      >
          <Pane 
            v-for="child in children" 
            :key="child.id" 
            :size="child.size || (100 / children.length)"
          >
              <SplitPaneLayout :node="child" />
          </Pane>
      </Splitpanes>
  </div>
</template>

<style>
/* Override splitpanes theme to match dark mode */
.splitpanes.default-theme .splitpanes__splitter {
    background-color: #44475a;
    border: none;
}
.splitpanes.default-theme .splitpanes__splitter:hover {
    background-color: #6272a4;
}

.splitpanes--vertical > .splitpanes__splitter {
    width: 4px;
}

.splitpanes--horizontal > .splitpanes__splitter {
    height: 4px;
}
</style>
