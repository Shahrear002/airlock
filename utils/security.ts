
// Utility for client-side encryption using Web Crypto API
// Uses AES-GCM with a locally generated key stored in localStorage.

const KEY_STORAGE_NAME = 'airlock_master_key';

async function getOrGenerateKey(): Promise<CryptoKey> {
    const storedKey = localStorage.getItem(KEY_STORAGE_NAME);

    if (storedKey) {
        // Import existing key
        const keyData = Uint8Array.from(atob(storedKey), c => c.charCodeAt(0));
        return await window.crypto.subtle.importKey(
            "raw",
            keyData,
            { name: "AES-GCM" },
            true,
            ["encrypt", "decrypt"]
        );
    } else {
        // Generate new key
        const key = await window.crypto.subtle.generateKey(
            { name: "AES-GCM", length: 256 },
            true,
            ["encrypt", "decrypt"]
        );

        // Export and store
        const exported = await window.crypto.subtle.exportKey("raw", key);
        const keyString = btoa(String.fromCharCode(...new Uint8Array(exported)));
        localStorage.setItem(KEY_STORAGE_NAME, keyString);

        return key;
    }
}

export async function encrypt(text: string): Promise<string> {
    if (!text) return '';
    try {
        const key = await getOrGenerateKey();
        const iv = window.crypto.getRandomValues(new Uint8Array(12));
        const encodedText = new TextEncoder().encode(text);

        const encryptedContent = await window.crypto.subtle.encrypt(
            { name: "AES-GCM", iv: iv },
            key,
            encodedText
        );

        // Combine IV and Ciphertext for storage: IV_BASE64:CIPHER_BASE64
        const ivStr = btoa(String.fromCharCode(...iv));
        const cipherStr = btoa(String.fromCharCode(...new Uint8Array(encryptedContent)));

        return `${ivStr}:${cipherStr}`;
    } catch (e) {
        console.error("Encryption failed:", e);
        throw e;
    }
}

export async function decrypt(encryptedData: string): Promise<string> {
    if (!encryptedData) return '';
    try {
        const [ivStr, cipherStr] = encryptedData.split(':');
        if (!ivStr || !cipherStr) throw new Error("Invalid encrypted format");

        const key = await getOrGenerateKey();
        const iv = Uint8Array.from(atob(ivStr), c => c.charCodeAt(0));
        const cipherText = Uint8Array.from(atob(cipherStr), c => c.charCodeAt(0));

        const decryptedContent = await window.crypto.subtle.decrypt(
            { name: "AES-GCM", iv: iv },
            key,
            cipherText
        );

        return new TextDecoder().decode(decryptedContent);
    } catch (e) {
        console.error("Decryption failed:", e);
        // Return empty or throw, depending on preference. For now, empty string implies generic failure or empty data.
        return '';
    }
}
