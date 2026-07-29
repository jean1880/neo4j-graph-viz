import { computed, markRaw, ref, shallowRef } from 'vue'
import type { GraphData, GraphLink, GraphNode, Neighbour } from '../types'

// Singleton shared state — every component that calls useGraph() sees the same graph,
// selection, hover, search query, and hidden-label set (no prop drilling for a small app).
//
// `data` is a shallowRef over a markRaw'd payload on purpose: d3-force and force-graph mutate
// every node (x/y/vx/vy/__indexColor) and rewrite each link's source/target in place, ~60×/s.
// Deep reactivity over that is both a hot-path proxy tax and a source of spurious computed
// invalidation. The graph is only ever replaced wholesale, so identity reactivity is enough.
const data = shallowRef<GraphData>(markRaw({ nodes: [], links: [] }))
const loading = ref(false)
const error = ref<string | null>(null)
const hidden = ref<Set<string>>(new Set())
const query = ref('')

// --- interaction state ---------------------------------------------------------------------
// Exactly two inputs drive every highlight in the app:
//   selectedId — sticky; only an explicit click, neighbour jump, or search hit changes it.
//   hoveredId  — transient preview; owned by the canvas, which decides when a reported hover
//                is trustworthy (force-graph re-runs hit detection every frame and reports a
//                stream of hover-out events during zoom/pan — see GraphCanvas).
// Everything visual derives from `focusId`, so a hover that ends falls back to the selection
// instead of dropping the highlight. That is what keeps a clicked node's neighbourhood lit
// through a zoom, a pan, a drag, or the cursor leaving the window.
const selectedId = ref<string | null>(null)
const hoveredId = ref<string | null>(null)

/** One-shot "bring this node into view" intent, consumed by the canvas. */
const revealRequest = ref<{ id: string; seq: number } | null>(null)
let revealSeq = 0

const EMPTY_IDS: ReadonlySet<string> = new Set()
const EMPTY_LINKS: ReadonlySet<GraphLink> = new Set()

/** Resolve a link end to a node id (force-graph swaps ids for node objects once simulating). */
function linkEnd(end: string | GraphNode): string {
  return typeof end === 'object' ? end.id : end
}

const nodeById = computed(() => new Map(data.value.nodes.map((n) => [n.id, n])))

/**
 * Incident links per node id, built once per graph load. Hover can fire on every animation
 * frame, so the neighbourhood lookup must not be an O(links) scan.
 */
const linksByNode = computed(() => {
  const m = new Map<string, GraphLink[]>()
  const add = (id: string, l: GraphLink) => {
    const arr = m.get(id)
    if (arr) arr.push(l)
    else m.set(id, [l])
  }
  for (const l of data.value.links) {
    const s = linkEnd(l.source)
    const t = linkEnd(l.target)
    add(s, l)
    if (t !== s) add(t, l)
  }
  return m
})

/** The node the highlight is currently built around: a live hover, else the selection. */
const focusId = computed(() => hoveredId.value ?? selectedId.value)

const focusNode = computed<GraphNode | null>(() =>
  focusId.value ? (nodeById.value.get(focusId.value) ?? null) : null,
)

/** The focused node plus its neighbours — everything outside this set renders dimmed. */
const focusNodeIds = computed<ReadonlySet<string>>(() => {
  const node = focusNode.value
  if (!node) return EMPTY_IDS
  const ids = new Set<string>([node.id])
  for (const l of linksByNode.value.get(node.id) ?? []) {
    ids.add(linkEnd(l.source))
    ids.add(linkEnd(l.target))
  }
  return ids
})

const focusLinks = computed<ReadonlySet<GraphLink>>(() => {
  const node = focusNode.value
  if (!node) return EMPTY_LINKS
  return new Set(linksByNode.value.get(node.id) ?? [])
})

/** True when the focus came from a live hover rather than the selection. */
const focusFromHover = computed(() => hoveredId.value !== null && focusNode.value !== null)

/** `[label, count]` pairs, sorted by count descending (for the legend). */
const counts = computed(() => {
  const c = new Map<string, number>()
  for (const n of data.value.nodes) c.set(n.label, (c.get(n.label) ?? 0) + 1)
  return [...c.entries()].sort((a, b) => b[1] - a[1])
})

const stats = computed(
  () => `${data.value.nodes.length} nodes · ${data.value.links.length} links`,
)

const selectedNode = computed<GraphNode | null>(() =>
  selectedId.value ? (nodeById.value.get(selectedId.value) ?? null) : null,
)

const hoveredNode = computed<GraphNode | null>(() =>
  hoveredId.value ? (nodeById.value.get(hoveredId.value) ?? null) : null,
)

/** A node can hold focus only while it still exists and its label is not hidden. */
function focusable(id: string | null): boolean {
  if (!id) return false
  const n = nodeById.value.get(id)
  return n !== undefined && !hidden.value.has(n.label)
}

/** Drop any selection/hover that a reload or a legend toggle just invalidated. */
function reconcileFocus() {
  if (!focusable(selectedId.value)) selectedId.value = null
  if (!focusable(hoveredId.value)) hoveredId.value = null
}

export function useGraph() {
  async function load() {
    loading.value = true
    error.value = null
    try {
      const res = await fetch('/api/graph')
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      data.value = markRaw((await res.json()) as GraphData)
      reconcileFocus()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      loading.value = false
    }
  }

  const isHidden = (label: string) => hidden.value.has(label)

  function toggleLabel(label: string) {
    const next = new Set(hidden.value)
    if (next.has(label)) next.delete(label)
    else next.add(label)
    hidden.value = next
    // Hiding a label must not leave the detail panel, the highlight, or the camera pointing at
    // a node that is no longer on the canvas.
    reconcileFocus()
  }

  /**
   * Isolate one label: hide every other type. Re-running it on the already-isolated label
   * restores everything, so the same gesture is its own undo.
   */
  function soloLabel(label: string) {
    const others = counts.value.filter(([l]) => l !== label).map(([l]) => l)
    const alreadySolo =
      others.length === hidden.value.size && others.every((l) => hidden.value.has(l))
    hidden.value = alreadySolo ? new Set() : new Set(others)
    reconcileFocus()
  }

  /** Un-hide every type. */
  function showAllLabels() {
    hidden.value = new Set()
  }

  const visible = (node: GraphNode) => !hidden.value.has(node.label)

  /** Ask the canvas to bring a node into view. Idempotent selections still re-centre. */
  function reveal(id: string) {
    revealRequest.value = { id, seq: ++revealSeq }
  }

  /**
   * Select a node. `reveal` is opt-in because a click on the canvas already has the node under
   * the cursor — moving the camera there would yank the view out from under the user (and, since
   * force-graph treats any transform change as a pointer drag, would also cancel the hover).
   * Jumps that originate off-canvas (neighbour chips, search) pass `reveal: true`.
   */
  function select(node: GraphNode, opts: { reveal?: boolean } = {}) {
    selectedId.value = node.id
    if (opts.reveal) reveal(node.id)
  }

  function clearSelection() {
    selectedId.value = null
  }

  /** Publish a hover. The canvas is the only caller — it owns when a hover is trustworthy. */
  function setHover(id: string | null) {
    hoveredId.value = id !== null && focusable(id) ? id : null
  }

  function neighboursOf(node: GraphNode): Neighbour[] {
    const res: Neighbour[] = []
    for (const l of linksByNode.value.get(node.id) ?? []) {
      const s = linkEnd(l.source)
      const t = linkEnd(l.target)
      if (s === node.id) res.push({ node: nodeById.value.get(t), type: l.type, dir: '→' })
      else res.push({ node: nodeById.value.get(s), type: l.type, dir: '←' })
    }
    return res
  }

  return {
    data,
    loading,
    error,
    hidden,
    query,
    selectedId,
    selectedNode,
    hoveredId,
    hoveredNode,
    focusId,
    focusNode,
    focusNodeIds,
    focusLinks,
    focusFromHover,
    revealRequest,
    nodeById,
    counts,
    stats,
    load,
    isHidden,
    toggleLabel,
    soloLabel,
    showAllLabels,
    visible,
    select,
    clearSelection,
    setHover,
    reveal,
    neighboursOf,
    linkEnd,
  }
}
