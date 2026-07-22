import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Dev: proxy /api to the local Rust backend (view.sh runs it on 127.0.0.1:8901).
// Build: emit to dist/, which the Rust `serve` layer serves in the container.
export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8901',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
