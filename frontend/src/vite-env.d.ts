/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Build-time viewer title (defaults to "Graph Viewer"). */
  readonly VITE_APP_TITLE?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
