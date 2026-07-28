import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { BaseDirectory, readTextFile, writeTextFile, exists, mkdir } from '@tauri-apps/plugin-fs'

export interface TerminalTheme {
    background: string
    foreground: string
    cursor: string
    cursorAccent: string
    selectionBackground: string
    black: string
    red: string
    green: string
    yellow: string
    blue: string
    magenta: string
    cyan: string
    white: string
    brightBlack: string
    brightRed: string
    brightGreen: string
    brightYellow: string
    brightBlue: string
    brightMagenta: string
    brightCyan: string
    brightWhite: string
}

export const builtinThemes: Record<string, TerminalTheme> = {
    'Dracula': {
        background: '#282a36',
        foreground: '#f8f8f2',
        cursor: '#f8f8f2',
        cursorAccent: '#282a36',
        selectionBackground: '#44475a',
        black: '#21222c',
        red: '#ff5555',
        green: '#50fa7b',
        yellow: '#f1fa8c',
        blue: '#bd93f9',
        magenta: '#ff79c6',
        cyan: '#8be9fd',
        white: '#f8f8f2',
        brightBlack: '#6272a4',
        brightRed: '#ff6e6e',
        brightGreen: '#69ff94',
        brightYellow: '#ffffa5',
        brightBlue: '#d6acff',
        brightMagenta: '#ff92df',
        brightCyan: '#a4ffff',
        brightWhite: '#ffffff',
    },
    'One Dark': {
        background: '#282c34',
        foreground: '#abb2bf',
        cursor: '#528bff',
        cursorAccent: '#282c34',
        selectionBackground: '#3e4451',
        black: '#282c34',
        red: '#e06c75',
        green: '#98c379',
        yellow: '#e5c07b',
        blue: '#61afef',
        magenta: '#c678dd',
        cyan: '#56b6c2',
        white: '#abb2bf',
        brightBlack: '#5c6370',
        brightRed: '#e06c75',
        brightGreen: '#98c379',
        brightYellow: '#e5c07b',
        brightBlue: '#61afef',
        brightMagenta: '#c678dd',
        brightCyan: '#56b6c2',
        brightWhite: '#ffffff',
    },
    'Monokai': {
        background: '#272822',
        foreground: '#f8f8f2',
        cursor: '#f8f8f0',
        cursorAccent: '#272822',
        selectionBackground: '#49483e',
        black: '#272822',
        red: '#f92672',
        green: '#a6e22e',
        yellow: '#f4bf75',
        blue: '#66d9ef',
        magenta: '#ae81ff',
        cyan: '#a1efe4',
        white: '#f8f8f2',
        brightBlack: '#75715e',
        brightRed: '#f92672',
        brightGreen: '#a6e22e',
        brightYellow: '#f4bf75',
        brightBlue: '#66d9ef',
        brightMagenta: '#ae81ff',
        brightCyan: '#a1efe4',
        brightWhite: '#f9f8f5',
    },
    'Solarized Dark': {
        background: '#002b36',
        foreground: '#839496',
        cursor: '#93a1a1',
        cursorAccent: '#002b36',
        selectionBackground: '#073642',
        black: '#073642',
        red: '#dc322f',
        green: '#859900',
        yellow: '#b58900',
        blue: '#268bd2',
        magenta: '#d33682',
        cyan: '#2aa198',
        white: '#eee8d5',
        brightBlack: '#002b36',
        brightRed: '#cb4b16',
        brightGreen: '#586e75',
        brightYellow: '#657b83',
        brightBlue: '#839496',
        brightMagenta: '#6c71c4',
        brightCyan: '#93a1a1',
        brightWhite: '#fdf6e3',
    },
    'Campbell': {
        background: '#0C0C0C',
        foreground: '#CCCCCC',
        cursor: '#FFFFFF',
        cursorAccent: '#0C0C0C',
        selectionBackground: '#FFFFFF',
        black: '#0C0C0C',
        red: '#C50F1F',
        green: '#13A10E',
        yellow: '#C19C00',
        blue: '#0037DA',
        magenta: '#881798',
        cyan: '#3A96DD',
        white: '#CCCCCC',
        brightBlack: '#767676',
        brightRed: '#E74856',
        brightGreen: '#16C60C',
        brightYellow: '#F9F1A5',
        brightBlue: '#3B78FF',
        brightMagenta: '#B4009E',
        brightCyan: '#61D6D6',
        brightWhite: '#F2F2F2',
    },
    'Campbell PowerShell': {
        background: '#012456',
        foreground: '#CCCCCC',
        cursor: '#FFFFFF',
        cursorAccent: '#012456',
        selectionBackground: '#FFFFFF',
        black: '#0C0C0C',
        red: '#C50F1F',
        green: '#13A10E',
        yellow: '#C19C00',
        blue: '#0037DA',
        magenta: '#881798',
        cyan: '#3A96DD',
        white: '#CCCCCC',
        brightBlack: '#767676',
        brightRed: '#E74856',
        brightGreen: '#16C60C',
        brightYellow: '#F9F1A5',
        brightBlue: '#3B78FF',
        brightMagenta: '#B4009E',
        brightCyan: '#61D6D6',
        brightWhite: '#F2F2F2',
    }
}

export const useSettingsStore = defineStore('settings', () => {
    const appTheme = ref<'default' | 'glass'>('default')
    const terminalThemeName = ref<string>('Dracula')
    const userThemes = ref<Record<string, TerminalTheme>>({})

    const allThemes = computed(() => {
        return { ...builtinThemes, ...userThemes.value }
    })

    const currentTheme = computed(() => {
        return allThemes.value[terminalThemeName.value] || builtinThemes['Dracula']
    })

    function setTerminalTheme(name: string) {
        if (allThemes.value[name]) {
            terminalThemeName.value = name
        }
    }

    async function loadUserThemes() {
        try {
            const hasFile = await exists('themes.json', { baseDir: BaseDirectory.AppConfig })
            if (hasFile) {
                const content = await readTextFile('themes.json', { baseDir: BaseDirectory.AppConfig })
                userThemes.value = JSON.parse(content)
            }
        } catch (e) {
            console.error('Failed to load user themes:', e)
        }
    }

    async function importTheme(name: string, theme: TerminalTheme) {
        userThemes.value[name] = theme
        // Persist to disk
        try {
            // Ensure config dir exists
            const configExists = await exists('', { baseDir: BaseDirectory.AppConfig })
            if (!configExists) {
                // mkdir recursive not available on root? usually AppConfig dir exists or we create it
                // Just try creating it if it fails
                await mkdir('', { baseDir: BaseDirectory.AppConfig, recursive: true })
            }

            await writeTextFile('themes.json', JSON.stringify(userThemes.value, null, 2), { baseDir: BaseDirectory.AppConfig })
            setTerminalTheme(name)
            return true
        } catch (e) {
            console.error('Failed to save themes.json:', e)
            throw e
        }
    }

    // Initialize
    loadUserThemes()

    return {
        appTheme,
        terminalThemeName,
        currentTheme,
        userThemes,
        allThemes,
        setTerminalTheme,
        loadUserThemes,
        importTheme
    }
}, {
    persist: true
})
