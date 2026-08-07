<script setup lang="ts">
import { useGraph } from '../composables/useGraph'
import { useSearch } from '../composables/useSearch'
import TypeLegend from './TypeLegend.vue'

const { stats, loading, error, load } = useGraph()
const { query, searchResult, searching, searchError, breadth, runSearch, clearSearch } =
  useSearch()

// Changing breadth re-runs the current query — the knob is meaningless if you have to retype.
function onBreadth() {
  if (query.value.trim()) void runSearch(query.value)
}

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

    <!-- Search state. Unlike the legend, search hits the server and can fail, come back empty,
         or be cut short — each of which the user has to be told about rather than left guessing
         why the canvas looks the way it does. -->
    <div v-if="query.trim()" class="srch">
      <span v-if="searching" class="muted">searching…</span>
      <span v-else-if="searchError" class="err">⚠ {{ searchError }}</span>
      <template v-else-if="searchResult">
        <span v-if="searchResult.visible.length === 0" class="muted">no matches</span>
        <span v-else>
          {{ searchResult.visible.length }} shown ·
          {{ searchResult.matches.length }} match{{ searchResult.matches.length === 1 ? '' : 'es' }}
          <span v-if="searchResult.semantic" class="tagged" title="Embedding similarity contributed">
            semantic
          </span>
          <span v-if="searchResult.truncated" class="warn" title="Result hit the server ceiling">
            truncated
          </span>
        </span>
        <button type="button" class="clear" @click="clearSearch">clear</button>
      </template>

      <label class="breadth">
        breadth
        <input
          v-model.number="breadth"
          type="range"
          min="0"
          max="1"
          step="0.05"
          aria-label="How far relatedness spreads from a match"
          @change="onBreadth"
        />
      </label>
    </div>

    <TypeLegend />
  </div>
</template>

<style scoped>
.srch {
  font-size: var(--text-sm);
  color: var(--text-muted);
  margin-top: 6px;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.srch .muted {
  color: var(--text-dim);
}
.srch .err {
  color: var(--danger, #f87171);
}
.tagged,
.warn {
  padding: 0 5px;
  border-radius: 20px;
  border: 1px solid var(--border-alt);
  font-size: var(--text-xs);
}
.warn {
  color: var(--warning, #fbbf24);
}
.clear {
  border: none;
  background: transparent;
  color: var(--primary);
  cursor: pointer;
  font: inherit;
  padding: 0;
  text-decoration: underline;
}
.breadth {
  display: flex;
  align-items: center;
  gap: 6px;
  inline-size: 100%;
  color: var(--text-dim);
}
.breadth input {
  flex: 1;
  min-inline-size: 0;
}
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
  font-size: var(--text-sm);
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
