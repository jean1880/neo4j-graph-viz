<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import GraphCanvas from './components/GraphCanvas.vue'
import HudPanel from './components/HudPanel.vue'
import NodeDetail from './components/NodeDetail.vue'
import ViewModeToggle from './components/ViewModeToggle.vue'
import { useGraph } from './composables/useGraph'

// The tooltip reads the shared hover state rather than keeping its own copy — one owner for
// "what is hovered" means the tooltip can never disagree with the canvas highlight.
const { load, clearSelection, hoveredNode } = useGraph()

const tipX = ref(0)
const tipY = ref(0)

/**
 * Tooltip position, coalesced to one update per frame.
 *
 * A pointer emits moves faster than the display refreshes — on a 1000 Hz mouse, several times
 * faster. Writing the refs per event queued a Vue update and an inline-style write for positions
 * that were overwritten before anything was painted. The handler now only records the newest
 * coordinates; the frame publishes them.
 */
let pointerX = 0
let pointerY = 0
let tipFrame = 0

function publishTip() {
  tipFrame = 0
  tipX.value = pointerX + 14
  tipY.value = pointerY + 14
}

function onMouseMove(e: MouseEvent) {
  pointerX = e.clientX
  pointerY = e.clientY
  if (!tipFrame) tipFrame = requestAnimationFrame(publishTip)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') clearSelection()
}

onMounted(() => {
  void load()
  window.addEventListener('mousemove', onMouseMove)
  window.addEventListener('keydown', onKeydown)
})
onBeforeUnmount(() => {
  if (tipFrame) cancelAnimationFrame(tipFrame)
  window.removeEventListener('mousemove', onMouseMove)
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <GraphCanvas />
  <HudPanel />
  <ViewModeToggle />
  <NodeDetail />

  <div v-if="hoveredNode" class="tip" :style="{ left: `${tipX}px`, top: `${tipY}px` }">
    <b>{{ hoveredNode.name }}</b
    ><br /><span>{{ hoveredNode.label }} · deg {{ hoveredNode.deg }}</span>
  </div>

  <div class="hint">
    scroll = zoom · drag = pan · click = pin · right-click = fit · esc = clear
  </div>
</template>

<style scoped>
.tip {
  position: fixed;
  z-index: 20;
  pointer-events: none;
  padding: 5px 9px;
  background: var(--bg-code);
  border: 1px solid var(--border-alt);
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  max-width: 260px;
  color: var(--text);
}
.tip b {
  color: var(--text);
}
.tip span {
  color: var(--text-muted);
}
.hint {
  position: fixed;
  bottom: 12px;
  right: var(--space-4);
  color: var(--text-dim);
  font-size: var(--text-sm);
  z-index: 10;
}
</style>
