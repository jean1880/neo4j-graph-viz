import { computed, ref } from 'vue'
import type { GraphData, GraphNode, Neighbour } from '../types'

// Singleton shared state — every component that calls useGraph() sees the same graph,
// selection, search query, and hidden-label set (no prop drilling for a small app).
const data = ref<GraphData>({ nodes: [], links: [] })
const loading = ref(false)
const error = ref<string | null>(null)
const hidden = ref<Set<string>>(new Set())
const query = ref('')
const selectedId = ref<string | null>(null)

/** Resolve a link end to a node id (force-graph swaps ids for node objects once simulating). */
function linkEnd(end: string | GraphNode): string {
  return typeof end === 'object' ? end.id : end
}

const nodeById = computed(() => new Map(data.value.nodes.map((n) => [n.id, n])))

const adjacency = computed(() => {
  const adj = new Map<string, Set<string>>()
  const add = (a: string, b: string) => {
    let set = adj.get(a)
    if (!set) {
      set = new Set()
      adj.set(a, set)
    }
    set.add(b)
  }
  for (const l of data.value.links) {
    const s = linkEnd(l.source)
    const t = linkEnd(l.target)
    add(s, t)
    add(t, s)
  }
  return adj
})

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

export function useGraph() {
  async function load() {
    loading.value = true
    error.value = null
    try {
      const res = await fetch('/api/graph')
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      data.value = (await res.json()) as GraphData
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
  }

  const visible = (node: GraphNode) => !hidden.value.has(node.label)

  const select = (node: GraphNode) => {
    selectedId.value = node.id
  }
  const clearSelection = () => {
    selectedId.value = null
  }

  function neighboursOf(node: GraphNode): Neighbour[] {
    const res: Neighbour[] = []
    for (const l of data.value.links) {
      const s = linkEnd(l.source)
      const t = linkEnd(l.target)
      if (s === node.id) res.push({ node: nodeById.value.get(t), type: l.type, dir: '→' })
      else if (t === node.id) res.push({ node: nodeById.value.get(s), type: l.type, dir: '←' })
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
    nodeById,
    adjacency,
    counts,
    stats,
    load,
    isHidden,
    toggleLabel,
    visible,
    select,
    clearSelection,
    neighboursOf,
    linkEnd,
  }
}
