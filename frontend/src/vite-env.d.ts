/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Starting view mode: '2' (default) or '3'. */
  readonly VITE_DEFAULT_VIEW_MODE?: string
  /** Build-time viewer title (defaults to "Graph Viewer"). */
  readonly VITE_APP_TITLE?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
