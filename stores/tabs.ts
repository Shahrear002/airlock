import { defineStore } from 'pinia'
import { ref } from 'vue'
import { useSessionsStore } from './sessions'

export type LayoutNodeType = 'horizontal' | 'vertical' | 'leaf'

export interface LayoutNode {
    id: string
    type: LayoutNodeType
    children?: LayoutNode[]
    sessionId?: string // Only for 'leaf'
    connectionId?: string // The ID used for the backend connection (can be shared)
    paneType?: 'terminal' | 'sftp' // What kind of pane is this?
    size?: number // Percentage for splitpanes
}

export interface Tab {
    id: string
    label: string
    root: LayoutNode
}

export const useTabsStore = defineStore('tabs', () => {
    const tabs = ref<Tab[]>([])
    const activeTabId = ref<string | null>(null)
    const activePaneId = ref<string | null>(null) // This corresponds to the sessionId in the active pane
    const sessionsStore = useSessionsStore()

    function createTab(sessionId: string, label: string) {
        const tabId = `tab-${Date.now()}`
        const newTab: Tab = {
            id: tabId,
            label,
            root: {
                id: `node-${Date.now()}`,
                type: 'leaf',
                sessionId,
                connectionId: sessionId,
                paneType: 'terminal'
            }
        }
        tabs.value.push(newTab)
        activeTabId.value = tabId
        activePaneId.value = sessionId
    }

    function getAllSessionIds(node: LayoutNode): string[] {
        if (node.sessionId) return [node.sessionId]
        let ids: string[] = []
        if (node.children) {
            for (const child of node.children) {
                ids = ids.concat(getAllSessionIds(child))
            }
        }
        return ids
    }

    function closeTab(tabId: string) {
        const index = tabs.value.findIndex(t => t.id === tabId)
        if (index !== -1) {
            // Clean up sessions
            const sessionIds = getAllSessionIds(tabs.value[index].root)
            sessionIds.forEach(id => {
                sessionsStore.removeSession(id)
            })

            tabs.value.splice(index, 1)
            // If active tab closed, switch to another
            if (activeTabId.value === tabId) {
                activeTabId.value = tabs.value.length > 0 ? tabs.value[tabs.value.length - 1].id : null
            }
        }
    }

    function findNodeAndParent(root: LayoutNode, targetId: string, parent: LayoutNode | null = null): { node: LayoutNode, parent: LayoutNode | null } | null {
        if (root.id === targetId) return { node: root, parent }
        if (root.children) {
            for (const child of root.children) {
                const result = findNodeAndParent(child, targetId, root)
                if (result) return result
            }
        }
        return null
    }

    function findNodeBySessionId(root: LayoutNode, sessionId: string, parent: LayoutNode | null = null): { node: LayoutNode, parent: LayoutNode | null } | null {
        if (root.sessionId === sessionId) return { node: root, parent }
        if (root.children) {
            for (const child of root.children) {
                const result = findNodeBySessionId(child, sessionId, root)
                if (result) return result
            }
        }
        return null
    }

    function splitPane(targetSessionId: string, direction: 'horizontal' | 'vertical', newSessionId: string, paneType: 'terminal' | 'sftp' = 'terminal', connectionId?: string) {
        const tab = tabs.value.find(t => t.id === activeTabId.value)
        if (!tab) return

        const result = findNodeBySessionId(tab.root, targetSessionId)
        if (!result) return

        const { node, parent } = result

        // Convert leaf to a group
        const originalSessionId = node.sessionId

        // New structure:
        // Group (direction)
        //   -> Leaf (original)
        //   -> Leaf (new)

        const originalPaneType = node.paneType || 'terminal'
        const originalConnectionId = node.connectionId || originalSessionId

        node.type = direction
        node.sessionId = undefined
        node.connectionId = undefined
        node.paneType = undefined
        node.children = [
            {
                id: `node-${Date.now()}-1`,
                type: 'leaf',
                sessionId: originalSessionId,
                connectionId: originalConnectionId,
                paneType: originalPaneType,
                size: 50
            },
            {
                id: `node-${Date.now()}-2`,
                type: 'leaf',
                sessionId: newSessionId,
                connectionId: connectionId || newSessionId,
                paneType: paneType,
                size: 50
            }
        ]

        activePaneId.value = newSessionId
    }

    function removePane(sessionId: string) {
        // Find which tab has this session
        for (const tab of tabs.value) {
            const result = findNodeBySessionId(tab.root, sessionId)
            if (result) {
                const { node, parent } = result

                if (!parent) {
                    // Removing root node means closing the tab
                    closeTab(tab.id)
                    return
                }

                // Remove child from parent
                const index = parent.children!.findIndex(c => c.id === node.id)
                parent.children!.splice(index, 1)

                // If parent has 0 children, remove parent (shouldn't happen with logic below)
                // If parent has 1 child, replace parent with child
                if (parent.children!.length === 1) {
                    const remainingChild = parent.children![0]

                    // Modify parent in place to become the child
                    parent.type = remainingChild.type
                    parent.sessionId = remainingChild.sessionId
                    parent.connectionId = remainingChild.connectionId
                    parent.paneType = remainingChild.paneType
                    parent.children = remainingChild.children
                    // parent.id = remainingChild.id 
                }

                // If the closed pane was active, switch activePane
                if (activePaneId.value === sessionId) {
                    activePaneId.value = null
                }
                return
            }
        }
    }

    function setActiveTab(id: string) {
        activeTabId.value = id
    }

    function setActivePane(id: string) {
        activePaneId.value = id
    }

    return {
        tabs,
        activeTabId,
        activePaneId,
        createTab,
        closeTab,
        splitPane,
        removePane,
        setActiveTab,
        setActivePane
    }
})
