<script setup lang="ts">
import { useGraph } from '../composables/useGraph'
import TypeLegend from './TypeLegend.vue'

const { stats, query, loading, error } = useGraph()

// Build-time branding: set VITE_APP_TITLE to rename the viewer without touching a component.
const title = import.meta.env.VITE_APP_TITLE || 'Graph Viewer'
</script>

<template>
  <div class="nv-graph-panel hud">
    <h1>{{ title }}</h1>
    <div class="sub">
      <span v-if="error" class="err">⚠ Failed to load: {{ error }}</span>
      <span v-else-if="loading">loading…</span>
      <span v-else>{{ stats }}</span>
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
  color: var(--text-muted);
  font-size: var(--text-xs);
  margin-bottom: var(--space-3);
}
.err {
  color: var(--error);
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
