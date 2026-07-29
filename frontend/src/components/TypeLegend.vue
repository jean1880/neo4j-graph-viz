<script setup lang="ts">
import { computed, ref } from 'vue'
import { useGraph } from '../composables/useGraph'
import { colorFor } from '../color'

const { counts, hidden, isHidden, toggleLabel, soloLabel, showAllLabels } = useGraph()

// Plain click toggles one type; ctrl/⌘-click isolates it (and isolating it again restores all).
// ⌘ is included because ctrl-click is a right-click on macOS.
const onLegendClick = (e: MouseEvent, label: string) => {
  if (e.ctrlKey || e.metaKey) soloLabel(label)
  else toggleLabel(label)
}

// Only count labels that are both hidden and actually present in the current graph, so the
// collapsed summary can never claim filters that aren't doing anything.
const hiddenCount = computed(() => counts.value.filter(([l]) => hidden.value.has(l)).length)

// The legend is the tallest thing in the HUD, so it starts collapsed where vertical space is
// scarce — on a phone it otherwise runs straight into the search box.
const open = ref(!window.matchMedia('(max-width: 640px)').matches)
const onToggle = (e: Event) => {
  open.value = (e.target as HTMLDetailsElement).open
}
</script>

<template>
  <details class="types" :open="open" @toggle="onToggle">
    <summary>
      <span class="chev" aria-hidden="true"></span>
      <h2>Types</h2>
      <span class="ct">{{ counts.length }}</span>
      <!-- Filters are invisible while collapsed; surface them so hidden types can't be forgotten.
           Kept a plain span — summary is itself a button, so it must not nest one. -->
      <span v-if="hiddenCount" class="off-ct">{{ hiddenCount }} off</span>
    </summary>
    <div class="items">
      <button
        v-for="[label, ct] in counts"
        :key="label"
        type="button"
        class="lg"
        :class="{ off: isHidden(label) }"
        :aria-pressed="!isHidden(label)"
        title="Click to show/hide · Ctrl-click to isolate this type"
        @click="onLegendClick($event, label)"
      >
        <span class="dot" :style="{ background: colorFor(label) }"></span>
        <span class="nm">{{ label }}</span>
        <span class="ct">{{ ct }}</span>
      </button>
    </div>
    <p class="tip">
      ctrl-click to isolate<template v-if="hiddenCount">
        ·
        <button type="button" class="reset" @click="showAllLabels()">show all</button>
      </template>
    </p>
  </details>
</template>

<style scoped>
.types {
  margin-top: var(--space-3);
  border-top: 1px solid var(--border-alt);
  padding-top: var(--space-2);
}
summary {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  cursor: pointer;
  padding: 2px 0;
  border-radius: var(--radius-sm);
  user-select: none;
  /* Replace the native disclosure marker with the chevron below. */
  list-style: none;
}
summary::-webkit-details-marker {
  display: none;
}
summary:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.chev {
  flex: 0 0 auto;
  inline-size: 0;
  block-size: 0;
  border-inline-start: 5px solid currentColor;
  border-block: 4px solid transparent;
  color: var(--text-dim);
  transition: rotate 160ms ease;
}
.types[open] .chev {
  rotate: 90deg;
}
h2 {
  margin: 0;
  flex: 1;
  font-size: var(--text-xs);
  text-transform: uppercase;
  letter-spacing: 0.8px;
  color: var(--text-muted);
  font-weight: 600;
}
.off-ct {
  font-size: 10px;
  color: var(--warning, var(--text-dim));
  font-variant-numeric: tabular-nums;
}
.tip {
  margin: var(--space-2) 0 0;
  font-size: 10px;
  color: var(--text-dim);
}
.reset {
  border: none;
  background: transparent;
  padding: 0;
  font: inherit;
  color: var(--primary);
  cursor: pointer;
  text-decoration: underline;
}
.items {
  /* The list scrolls inside the HUD rather than growing the panel off-screen. */
  max-block-size: min(46vh, 340px);
  overflow-y: auto;
  /* Filtering types changes the list length constantly; reserve the gutter so the rows don't
     shift sideways every time the scrollbar appears or goes away. */
  scrollbar-gutter: stable;
  margin-top: var(--space-2);
}
.lg {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: 3px 4px;
  border: none;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  border-radius: var(--radius-sm);
  cursor: pointer;
  user-select: none;
}
.lg:hover {
  background: color-mix(in srgb, var(--text) 5%, transparent);
}
.lg.off {
  opacity: 0.35;
}
.dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex: 0 0 auto;
}
.nm {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ct {
  color: var(--text-dim);
  font-variant-numeric: tabular-nums;
  font-size: var(--text-xs);
}

/* Progressive enhancement: browsers with ::details-content animate the open/close, the rest
   snap. `interpolate-size: allow-keywords` (set in style.css) is what makes `auto` tweenable. */
@media (prefers-reduced-motion: no-preference) {
  .types::details-content {
    block-size: 0;
    overflow: clip;
    transition:
      block-size 200ms ease,
      content-visibility 200ms;
    transition-behavior: allow-discrete;
  }
  .types[open]::details-content {
    block-size: auto;
  }
}
@media (prefers-reduced-motion: reduce) {
  .chev {
    transition: none;
  }
}
</style>
