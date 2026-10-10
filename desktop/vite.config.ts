import { readFileSync } from 'node:fs';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;
const pkg = JSON.parse(readFileSync('package.json', 'utf-8'));

// Release builds are stamped with their tag (e.g. v8.4.13-direct.2) via
// SCD_RELEASE_TAG so the app knows its own -direct build number.
// Dev builds have no tag and stay on the bare package version.
export default defineConfig(async () => ({
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
    // @ts-expect-error process is a nodejs global
    __DIRECT_TAG__: JSON.stringify(process.env.SCD_RELEASE_TAG ?? ''),
  },
  plugins: [react(), tailwindcss()],
  build: {
    rollupOptions: {
      input: {
        main: 'index.html',
      },
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return;
          if (id.includes('react-router')) return 'router';
          if (id.includes('@tanstack/react-query') || id.includes('@tanstack/react-virtual')) {
            return 'tanstack';
          }
          if (id.includes('@dnd-kit')) return 'dnd-kit';
          if (id.includes('@radix-ui')) return 'radix';
          if (id.includes('lucide-react') || id.includes('simple-icons')) return 'icons';
          if (id.includes('react-markdown')) return 'markdown';
          if (id.includes('@tauri-apps')) return 'tauri';
          if (
            id.includes('/node_modules/react/') ||
            id.includes('/node_modules/react-dom/') ||
            id.includes('/node_modules/scheduler/')
          ) {
            return 'react-vendor';
          }
        },
      },
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
}));
