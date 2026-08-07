<script setup lang="ts">
import { useViewMode } from '../composables/useViewMode'

const { dimensions, set } = useViewMode()
</script>

<template>
  <!-- A segmented control rather than a switch: with two named states there is no ambiguity
       about which one is active, which a bare toggle always has. -->
  <div class="nv-graph-panel modes" role="group" aria-label="View mode">
    <button
      type="button"
      :class="{ on: dimensions === 2 }"
      :aria-pressed="dimensions === 2"
      title="Flat view — best for reading structure"
      @click="set(2)"
    >
      2D
    </button>
    <button
      type="button"
      :class="{ on: dimensions === 3 }"
      :aria-pressed="dimensions === 3"
      title="Orbit view — drag to rotate. Clearer on small graphs than on the full map."
      @click="set(3)"
    >
      3D
    </button>
  </div>
</template>

<style scoped>
.modes {
  position: fixed;
  top: var(--space-4);
  right: var(--space-4);
  z-index: 15;
  display: flex;
  gap: 2px;
  padding: 3px;
}
button {
  font: inherit;
  font-size: var(--text-sm);
  font-weight: 600;
  letter-spacing: 0.4px;
  padding: 5px 14px;
  min-height: 26px;
  border: none;
  border-radius: var(--radius-sm, 5px);
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
  line-height: 1.5;
}
button:hover {
  color: var(--text);
  background: color-mix(in srgb, var(--text) 7%, transparent);
}
button.on {
  background: var(--primary);
  color: var(--bg, #020617);
}
button:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
</style>
