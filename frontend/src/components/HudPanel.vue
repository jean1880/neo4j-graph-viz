<script setup lang="ts">
import { useGraph } from '../composables/useGraph'
import { useSearch } from '../composables/useSearch'
import LayoutSettings from './LayoutSettings.vue'
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

const pill = 'rounded-full border border-border-strong px-1.25 text-xs'
</script>

<template>
  <!-- Never wider than the viewport allows — on a phone a fixed width would sit under the
       legend and overlap the detail panel. 264px is the border-box width: 230px of content. -->
  <div
    class="graph-panel top-4 left-4 z-10 px-4 py-3 max-h-[calc(100dvh-2rem)] w-[min(264px,calc(100vw-2rem))] overflow-y-auto"
  >
    <h1 class="mb-0.5 text-lg font-[650] tracking-[0.2px]">{{ title }}</h1>
    <div class="mb-3 flex items-center gap-2 text-sm text-fg-muted">
      <span v-if="error" class="text-error">⚠ Failed to load: {{ error }}</span>
      <span v-else-if="loading">loading…</span>
      <span v-else>{{ stats }}</span>
      <button
        type="button"
        class="ms-auto flex-none cursor-pointer rounded-sm px-1 py-0.5 text-sm leading-none text-fg-dim not-disabled:hover:bg-fg/8 not-disabled:hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-primary disabled:cursor-default disabled:opacity-40"
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
      class="w-full rounded-sm border border-border-strong bg-surface px-2.25 py-1.75 text-sm text-fg outline-none focus-visible:border-primary"
      placeholder="Search nodes…"
      autocomplete="off"
      aria-label="Search nodes"
    />

    <!-- Search state. Unlike the legend, search hits the server and can fail, come back empty,
         or be cut short — each of which the user has to be told about rather than left guessing
         why the canvas looks the way it does. -->
    <div v-if="query.trim()" class="mt-1.5 flex flex-wrap items-center gap-1.5 text-sm text-fg-muted">
      <span v-if="searching" class="text-fg-dim">searching…</span>
      <span v-else-if="searchError" class="text-error">⚠ {{ searchError }}</span>
      <template v-else-if="searchResult">
        <span v-if="searchResult.visible.length === 0" class="text-fg-dim">no matches</span>
        <span v-else>
          {{ searchResult.visible.length }} shown ·
          {{ searchResult.matches.length }} match{{ searchResult.matches.length === 1 ? '' : 'es' }}
          <span v-if="searchResult.semantic" :class="pill" title="Embedding similarity contributed">
            semantic
          </span>
          <span v-if="searchResult.truncated" :class="[pill, 'text-warning']" title="Result hit the server ceiling">
            truncated
          </span>
        </span>
        <button type="button" class="cursor-pointer text-primary underline" @click="clearSearch">clear</button>
      </template>

      <label class="flex w-full items-center gap-1.5 text-fg-dim">
        breadth
        <input
          v-model.number="breadth"
          class="min-w-0 flex-1"
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
    <LayoutSettings />
  </div>
</template>
