import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
    title: "Airlock",
    description: "A secure, local-first SSH client.",
    base: '/airlock/', // Important for GitHub Pages: https://shahrear002.github.io/airlock/

    themeConfig: {
        // https://vitepress.dev/reference/default-theme-config
        logo: '/logo.png', // We need to add a logo later, or use text
        nav: [
            { text: 'Home', link: '/' },
            { text: 'Guide', link: '/guide/getting-started' },
            { text: 'Download', link: 'https://github.com/Shahrear002/airlock/releases/latest' }
        ],

        sidebar: [
            {
                text: 'User Guide',
                items: [
                    { text: 'Getting Started', link: '/guide/getting-started' },
                    { text: 'Managing Hosts', link: '/guide/host-management' },
                    { text: 'Terminal Features', link: '/guide/terminal' },
                    { text: 'Security & Encryption', link: '/guide/security' },
                    { text: 'Import & Export', link: '/guide/backup' }
                ]
            }
        ],

        socialLinks: [
            { icon: 'github', link: 'https://github.com/Shahrear002/airlock' }
        ],

        footer: {
            message: 'Released under the MIT License.',
            copyright: 'Copyright © 2026 Shahrear'
        }
    }
})
