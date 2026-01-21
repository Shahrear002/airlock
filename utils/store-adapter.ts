

import { LazyStore } from '@tauri-apps/plugin-store';

// Initialize the store
// We use a specific filename for the store.
const store = new LazyStore('store.bin');

export const TauriStoreAdapter = {
    async getItem(key: string): Promise<string | null> {
        try {
            const val = await store.get<string>(key);
            return val || null;
        } catch (e) {
            console.error('[StoreAdapter] Failed to get item:', e);
            return null;
        }
    },
    async setItem(key: string, value: string): Promise<void> {
        try {
            const oldValue = await store.get<string>(key);

            // Safety Check: If we are trying to save an empty/default state, but the disk has data, bad things are happening.
            // Pinia hydration might be triggering a save of initial state before load completes.
            if (key === 'hosts' && value === '{"hosts":[]}' && oldValue && oldValue.length > 20) {
                console.warn(`[StoreAdapter] PREVENTING OVEWRITE: Attempted to save empty hosts over existing data (len ${oldValue.length}). Skipping.`);
                return;
            }

            if (oldValue !== value) {
                await store.set(key, value);
                await store.save(); // Ensure it's written to disk
            }
        } catch (e) {
            console.error('[StoreAdapter] Failed to set item:', e);
        }
    },
    async removeItem(key: string): Promise<void> {
        try {
            await store.delete(key);
            await store.save();
        } catch (e) {
            console.error('Failed to remove item from Tauri Store', e);
        }
    }
};
