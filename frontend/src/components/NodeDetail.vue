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

const section = 'mt-3.25 mb-1.5 text-xs tracking-wider text-fg-dim uppercase'
const skeletonRow = 'h-2.75 rounded-xs bg-fg/8 motion-safe:animate-skeleton'
</script>

<template>
  <!-- Node-to-node the content length swings a lot; a reserved gutter keeps the text from
       reflowing sideways as the scrollbar comes and goes. 334px is the border-box width:
       300px of content. -->
  <div
    v-if="selectedNode"
    class="graph-panel right-4 bottom-4 z-10 max-h-[62vh] w-[334px] overflow-y-auto px-4 py-3 [scrollbar-gutter:stable]"
  >
    <div class="mb-1 flex items-start gap-2">
      <span
        class="mt-0.75 size-2.75 flex-none rounded-full"
        :style="{ background: colorFor(selectedNode.label) }"
      ></span>
      <h3 class="flex-1 text-base font-[650] wrap-break-word">{{ selectedNode.name }}</h3>
      <!-- WCAG 2.5.8 asks for 24x24 CSS px on pointer targets; a bare glyph is well under it. -->
      <button
        type="button"
        class="inline-flex min-h-7 min-w-7 flex-none cursor-pointer items-center justify-center rounded-sm text-xl leading-none text-fg-muted hover:bg-fg/8 hover:text-fg"
        aria-label="Close node details"
        @click="clearSelection"
      >
        ×
      </button>
    </div>
    <!-- `group` is empty unless GRAPH_WRAPPER_LABELS matched a wrapper label — omit it then. -->
    <div class="mb-1.5 text-sm text-fg-muted">
      {{ selectedNode.label }} ·<template v-if="selectedNode.group">
        {{ selectedNode.group }} ·</template>
      {{ selectedNode.deg }} connection{{ selectedNode.deg === 1 ? '' : 's' }}
    </div>

    <div :class="section">Properties</div>
    <!-- Fetched per node, so this section has loading and error states the rest of the panel
         does not. Skeleton rows rather than a spinner keep the panel height from jumping. -->
    <div v-if="detailLoading" class="flex flex-col gap-1.5" aria-busy="true" aria-label="Loading properties">
      <span :class="skeletonRow"></span>
      <span :class="[skeletonRow, 'w-4/5']"></span>
      <span :class="[skeletonRow, 'w-[55%]']"></span>
    </div>
    <div v-else-if="detailError" class="text-sm text-fg-dim">
      could not load properties — {{ detailError }}
    </div>
    <table v-else class="w-full">
      <tbody class="[&_td]:py-0.75 [&_td]:align-top [&_td]:text-sm">
        <tr v-if="propRows.length === 0">
          <td class="text-fg-dim">no properties</td>
        </tr>
        <tr v-for="[k, v] in propRows" :key="k">
          <td class="w-[1%] pe-2.5 whitespace-nowrap text-fg-muted">{{ k }}</td>
          <td class="wrap-break-word text-fg">{{ v }}</td>
        </tr>
      </tbody>
    </table>

    <div :class="section">Connections</div>
    <div v-if="relSummary.length" class="mb-2 text-sm">
      <template v-for="([t, c], i) in relSummary" :key="t"
        ><span v-if="i > 0"> · </span>{{ t }} <span class="text-fg-dim">{{ c }}</span></template
      >
    </div>
    <div>
      <button
        v-for="(nb, i) in chips"
        :key="i"
        type="button"
        class="my-0.5 me-1 inline-block min-h-6 cursor-pointer rounded-full border border-border-strong bg-surface px-2.5 py-1 text-sm leading-[1.4] text-fg-muted hover:border-primary hover:text-fg"
        :title="`${nb.dir} ${nb.type}`"
        @click="goto(nb.node)"
      >
        {{ nb.node?.name ?? '?' }}
      </button>
      <div v-if="moreCount" class="mt-1.5 text-fg-dim">+{{ moreCount }} more…</div>
    </div>
  </div>
</template>
