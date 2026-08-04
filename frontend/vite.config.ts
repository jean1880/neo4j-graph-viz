import { defineConfig, loadEnv } from 'vite'
import vue from '@vitejs/plugin-vue'

// Dev: proxy /api to the local backend (view.sh runs it on 127.0.0.1:8901; override with
//      VITE_API_TARGET when the backend runs elsewhere).
// Build: emit to dist/, which the Rust `serve` layer serves in the container.
//
// Config is read from the repo-root .env (one file for both halves of the app), and only the
// VITE_ prefix is loaded — no non-prefixed variable can reach the client bundle.
const ENV_DIR = '..'

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, ENV_DIR, 'VITE_')
  const title = env.VITE_APP_TITLE || 'Graph Viewer'

  return {
    envDir: ENV_DIR,
    // Relative by default so ONE built image works at any mount path — '/', '/tools/graph/',
    // anything — without a rebuild. An absolute base would hardcode the deployment topology
    // into the bundle. Set VITE_BASE_PATH to pin an absolute base if your proxy needs it.
    base: env.VITE_BASE_PATH || './',
    plugins: [
      vue(),
      {
        // Keep the document title in step with the in-app heading. Vite's built-in %VAR%
        // substitution would leave a literal placeholder when the var is unset, so do it here
        // where a default is possible.
        name: 'html-title',
        transformIndexHtml: (html: string) =>
          html.replace(/<title>.*?<\/title>/, `<title>${title}</title>`),
      },
    ],
    server: {
      proxy: {
        '/api': env.VITE_API_TARGET || 'http://127.0.0.1:8901',
      },
    },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
    },
  }
})
