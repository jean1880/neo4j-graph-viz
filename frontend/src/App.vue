<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import GraphCanvas from './components/GraphCanvas.vue'
import HudPanel from './components/HudPanel.vue'
import NodeDetail from './components/NodeDetail.vue'
import { useGraph } from './composables/useGraph'

// The tooltip reads the shared hover state rather than keeping its own copy — one owner for
// "what is hovered" means the tooltip can never disagree with the canvas highlight.
const { load, clearSelection, hoveredNode } = useGraph()

const tipX = ref(0)
const tipY = ref(0)

function onMouseMove(e: MouseEvent) {
  tipX.value = e.clientX + 14
  tipY.value = e.clientY + 14
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
  window.removeEventListener('mousemove', onMouseMove)
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <GraphCanvas />
  <HudPanel />
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
  font-size: var(--text-xs);
  z-index: 10;
}
</style>
