<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { apiUrl } from '../api'
import { useGraph } from '../composables/useGraph'
import { colorFor } from '../color'
import type { GraphNode, NodeDetailData } from '../types'

const { selectedNode, selectedId, clearSelection, neighboursOf, select } = useGraph()

const neighbours = computed(() =>
  selectedNode.value ? neighboursOf(selectedNode.value) : [],
)

const relSummary = computed(() => {
  const byType = new Map<string, number>()
  for (const n of neighbours.value) byType.set(n.type, (byType.get(n.type) ?? 0) + 1)
  return [...byType.entries()].sort((a, b) => b[1] - a[1])
})

// --- lazily fetched properties ---------------------------------------------------------------
// Properties no longer travel with the graph payload (they dominated it at scale), so the panel
// fetches the selected node's own record. Everything else the panel shows — name, label, degree,
// neighbour chips — still comes from the graph and renders immediately; only this table waits.
const detail = ref<NodeDetailData | null>(null)
const detailError = ref<string | null>(null)
const detailLoading = ref(false)
// Selections can change faster than the network answers (arrow-key or chip walking), and
// responses are not guaranteed to arrive in order — so each request cancels the one before it.
let inFlight: AbortController | null = null

async function loadDetail(id: string | null) {
  inFlight?.abort()
  inFlight = null
  detail.value = null
  detailError.value = null
  if (!id) {
    detailLoading.value = false
    return
  }

  const ctrl = new AbortController()
  inFlight = ctrl
  detailLoading.value = true
  try {
    const res = await fetch(apiUrl(`api/node/${encodeURIComponent(id)}`), {
      signal: ctrl.signal,
    })
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const body = (await res.json()) as NodeDetailData
    // A late response for a node the user has already moved off must not overwrite the panel.
    if (ctrl.signal.aborted || selectedId.value !== id) return
    detail.value = body
  } catch (e) {
    if (ctrl.signal.aborted) return
    detailError.value = e instanceof Error ? e.message : String(e)
  } finally {
    if (inFlight === ctrl) {
      inFlight = null
      detailLoading.value = false
    }
  }
}

watch(selectedId, (id) => void loadDetail(id), { immediate: true })
onBeforeUnmount(() => inFlight?.abort())

const propRows = computed(() =>
  detail.value ? Object.entries(detail.value.props).filter(([k]) => k !== 'name') : [],
)

const chips = computed(() => neighbours.value.slice(0, 60))
const moreCount = computed(() => Math.max(0, neighbours.value.length - 60))

// Jumping from a chip is an off-canvas intent — the target is very likely off-screen, so this
// is one of the few paths that is allowed to move the camera.
function goto(node: GraphNode | undefined) {
  if (node) select(node, { reveal: true })
}
</script>

<template>
  <div v-if="selectedNode" class="nv-graph-panel detail">
    <div class="dhead">
      <span class="dot" :style="{ background: colorFor(selectedNode.label) }"></span>
      <h3>{{ selectedNode.name }}</h3>
      <button type="button" class="close" aria-label="Close node details" @click="clearSelection">
        ×
      </button>
    </div>
    <!-- `group` is empty unless GRAPH_WRAPPER_LABELS matched a wrapper label — omit it then. -->
    <div class="meta">
      {{ selectedNode.label }} ·<template v-if="selectedNode.group">
        {{ selectedNode.group }} ·</template>
      {{ selectedNode.deg }} connection{{ selectedNode.deg === 1 ? '' : 's' }}
    </div>

    <div class="sec">Properties</div>
    <!-- Fetched per node, so this section has loading and error states the rest of the panel
         does not. Skeleton rows rather than a spinner keep the panel height from jumping. -->
    <div v-if="detailLoading" class="skel" aria-busy="true" aria-label="Loading properties">
      <span v-for="i in 3" :key="i" class="skel-row"></span>
    </div>
    <div v-else-if="detailError" class="muted err">
      could not load properties — {{ detailError }}
    </div>
    <table v-else>
      <tbody>
        <tr v-if="propRows.length === 0">
          <td class="v muted">no properties</td>
        </tr>
        <tr v-for="[k, v] in propRows" :key="k">
          <td class="k">{{ k }}</td>
          <td class="v">{{ v }}</td>
        </tr>
      </tbody>
    </table>

    <div class="sec">Connections</div>
    <div v-if="relSummary.length" class="rels">
      <template v-for="([t, c], i) in relSummary" :key="t"
        ><span v-if="i > 0"> · </span>{{ t }} <span class="muted">{{ c }}</span></template
      >
    </div>
    <div>
      <button
        v-for="(nb, i) in chips"
        :key="i"
        type="button"
        class="nb"
        :title="`${nb.dir} ${nb.type}`"
        @click="goto(nb.node)"
      >
        {{ nb.node?.name ?? '?' }}
      </button>
      <div v-if="moreCount" class="muted more">+{{ moreCount }} more…</div>
    </div>
  </div>
</template>

<style scoped>
.detail {
  bottom: var(--space-4);
  right: var(--space-4);
  width: 300px;
  max-height: 62vh;
  overflow-y: auto;
  /* Node-to-node the content length swings a lot; a reserved gutter keeps the text from
     reflowing sideways as the scrollbar comes and goes. */
  scrollbar-gutter: stable;
}
.dhead {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  margin-bottom: 4px;
}
.dot {
  width: 11px;
  height: 11px;
  border-radius: 50%;
  flex: 0 0 auto;
  margin-top: 3px;
}
h3 {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 650;
  flex: 1;
  word-break: break-word;
}
.close {
  cursor: pointer;
  color: var(--text-muted);
  font-size: var(--text-xl);
  line-height: 1;
  /* WCAG 2.5.8 asks for 24x24 CSS px on pointer targets; a bare glyph is well under it. */
  min-width: 28px;
  min-height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
  border: none;
  background: transparent;
  border-radius: 5px;
  flex: 0 0 auto;
}
.close:hover {
  background: color-mix(in srgb, var(--text) 8%, transparent);
  color: var(--text);
}
.meta {
  color: var(--text-muted);
  font-size: var(--text-sm);
  margin-bottom: 6px;
}
.sec {
  text-transform: uppercase;
  letter-spacing: 0.8px;
  font-size: var(--text-xs);
  color: var(--text-dim);
  margin: 13px 0 6px;
}
table {
  width: 100%;
  border-collapse: collapse;
}
td {
  padding: 3px 0;
  vertical-align: top;
  font-size: var(--text-sm);
}
td.k {
  color: var(--text-muted);
  padding-right: 10px;
  white-space: nowrap;
  width: 1%;
}
td.v {
  color: var(--text);
  word-break: break-word;
}
.muted {
  color: var(--text-dim);
}
.err {
  font-size: var(--text-sm);
}
.skel {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.skel-row {
  height: 11px;
  border-radius: 3px;
  background: color-mix(in srgb, var(--text) 8%, transparent);
}
.skel-row:nth-child(2) {
  width: 80%;
}
.skel-row:nth-child(3) {
  width: 55%;
}
@media (prefers-reduced-motion: no-preference) {
  .skel-row {
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }
}
.rels {
  font-size: var(--text-sm);
  margin-bottom: var(--space-2);
}
.nb {
  display: inline-block;
  margin: 2px 4px 2px 0;
  padding: 4px 10px;
  min-height: 24px;
  border-radius: 20px;
  background: var(--surface);
  border: 1px solid var(--border-alt);
  color: var(--text-muted);
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  font-size: var(--text-sm);
}
.nb:hover {
  border-color: var(--primary);
  color: var(--text);
}
.more {
  margin-top: 6px;
}
</style>
