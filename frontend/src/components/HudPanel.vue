<script setup lang="ts">
import { useGraph } from '../composables/useGraph'
import TypeLegend from './TypeLegend.vue'

const { stats, query, loading, error, load } = useGraph()

// Build-time branding: set VITE_APP_TITLE to rename the viewer without touching a component.
const title = import.meta.env.VITE_APP_TITLE || 'Graph Viewer'

// The server caches the graph for GRAPH_CACHE_TTL_SECS, so a plain reload re-reads the same
// payload — this is the only way to pull fresh data before the TTL expires.
const refresh = () => load({ refresh: true })
</script>

<template>
  <div class="nv-graph-panel hud">
    <h1>{{ title }}</h1>
    <div class="sub">
      <span v-if="error" class="err">⚠ Failed to load: {{ error }}</span>
      <span v-else-if="loading">loading…</span>
      <span v-else>{{ stats }}</span>
      <button
        type="button"
        class="refresh"
        :disabled="loading"
        title="Refetch the graph, bypassing the server cache"
        aria-label="Refresh graph data"
        @click="refresh"
      >
        <span aria-hidden="true">⟳</span>
      </button>
    </div>
    <input
      v-model="query"
      type="search"
      class="search"
      placeholder="Search nodes…"
      autocomplete="off"
      aria-label="Search nodes"
    />
    <TypeLegend />
  </div>
</template>

<style scoped>
.hud {
  top: var(--space-4);
  left: var(--space-4);
  /* Never wider than the viewport allows — on a phone the fixed 230px used to sit under the
     legend and overlap the detail panel. */
  inline-size: min(230px, calc(100vw - var(--space-4) * 2));
  max-block-size: calc(100dvh - var(--space-4) * 2);
  overflow-y: auto;
}
h1 {
  margin: 0 0 2px;
  font-size: var(--text-lg);
  font-weight: 650;
  letter-spacing: 0.2px;
}
.sub {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-muted);
  font-size: var(--text-xs);
  margin-bottom: var(--space-3);
}
.err {
  color: var(--error);
}
.refresh {
  margin-inline-start: auto;
  flex: 0 0 auto;
  border: none;
  background: transparent;
  padding: 2px 4px;
  border-radius: var(--radius-sm);
  color: var(--text-dim);
  font: inherit;
  font-size: var(--text-sm);
  line-height: 1;
  cursor: pointer;
}
.refresh:hover:not(:disabled) {
  background: color-mix(in srgb, var(--text) 8%, transparent);
  color: var(--text);
}
.refresh:disabled {
  cursor: default;
  opacity: 0.4;
}
.refresh:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.search {
  width: 100%;
  padding: 7px 9px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-alt);
  background: var(--surface);
  color: var(--text);
  outline: none;
  font-family: var(--font-sans);
  font-size: var(--text-sm);
}
.search:focus-visible {
  border-color: var(--primary);
}
</style>
