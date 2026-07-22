<script setup lang="ts">
import { useGraph } from '../composables/useGraph'
import { colorFor } from '../color'

const { counts, isHidden, toggleLabel } = useGraph()
</script>

<template>
  <div class="nv-graph-panel legend">
    <h2>Types</h2>
    <button
      v-for="[label, ct] in counts"
      :key="label"
      type="button"
      class="lg"
      :class="{ off: isHidden(label) }"
      :aria-pressed="!isHidden(label)"
      @click="toggleLabel(label)"
    >
      <span class="dot" :style="{ background: colorFor(label) }"></span>
      <span class="nm">{{ label }}</span>
      <span class="ct">{{ ct }}</span>
    </button>
  </div>
</template>

<style scoped>
.legend {
  bottom: var(--space-4);
  left: var(--space-4);
  max-height: 46vh;
  overflow-y: auto;
  width: 210px;
}
h2 {
  margin: 0 0 var(--space-2);
  font-size: var(--text-xs);
  text-transform: uppercase;
  letter-spacing: 0.8px;
  color: var(--text-muted);
  font-weight: 600;
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
}
.ct {
  color: var(--text-dim);
  font-variant-numeric: tabular-nums;
}
</style>
