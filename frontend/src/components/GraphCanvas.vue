<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import ForceGraph, {
  type GraphData as FgGraphData,
  type LinkObject,
  type NodeObject,
} from 'force-graph'
import { useGraph } from '../composables/useGraph'
import { colorFor } from '../color'
import type { GraphLink, GraphNode } from '../types'

const el = ref<HTMLDivElement | null>(null)

const emit = defineEmits<{ hover: [node: GraphNode | null] }>()

const { data, hidden, query, selectedId, adjacency, nodeById, select, clearSelection, linkEnd } =
  useGraph()

// force-graph is imperative and owns its own render loop; we hold the instance and push
// reactive changes into it via watchers. Casts bridge force-graph's generic Node/Link objects
// to our concrete types (its objects carry our extra fields through an index signature).
let graph: ForceGraph | null = null

const hoverNodes = new Set<string>()
const hoverLinks = new Set<LinkObject>()

const asNode = (n: NodeObject) => n as unknown as GraphNode
const isVisible = (n: NodeObject) => !hidden.value.has(asNode(n).label)
const q = () => query.value.trim().toLowerCase()

// Honour reduced-motion for the discretionary camera moves (the force sim itself is inherent
// to the viz and stays). SETTLE_MS lets the simulation settle before the initial zoom-to-fit.
const anim = (ms: number) =>
  window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : ms
const SETTLE_MS = 700
let searchTimer: number | undefined

function applyVisibility() {
  graph?.nodeVisibility((n) => isVisible(n))
  graph?.linkVisibility(
    (l: LinkObject) => isVisible(l.source as NodeObject) && isVisible(l.target as NodeObject),
  )
}

function centerOn(id: string | null) {
  if (!id || !graph) return
  const n = nodeById.value.get(id)
  if (n && n.x != null && n.y != null) graph.centerAt(n.x, n.y, anim(400))
}

onMounted(() => {
  if (!el.value) return
  // Fallback must track @nuvek/ui's --bg default; only used if the token fails to resolve.
  const bg =
    getComputedStyle(document.documentElement).getPropertyValue('--bg').trim() || '#020617'

  graph = new ForceGraph(el.value)
    .graphData(data.value as unknown as FgGraphData)
    .backgroundColor(bg)
    .nodeId('id')
    .nodeVal((n) => 1 + Math.sqrt(asNode(n).deg))
    .nodeVisibility((n) => isVisible(n))
    .linkVisibility(
      (l: LinkObject) => isVisible(l.source as NodeObject) && isVisible(l.target as NodeObject),
    )
    .linkColor((l) => (hoverLinks.has(l) ? 'rgba(120,160,255,.9)' : 'rgba(150,155,180,.12)'))
    .linkWidth((l) => (hoverLinks.has(l) ? 1.6 : 0.5))
    .linkDirectionalParticles((l) => (hoverLinks.has(l) ? 2 : 0))
    .linkDirectionalParticleWidth(2)
    .nodeCanvasObject((node, ctx, scale) => {
      const n = asNode(node)
      const r = 2 + Math.sqrt(n.deg)
      const dim = hoverNodes.size > 0 && !hoverNodes.has(n.id)
      const match = q() !== '' && n.name.toLowerCase().includes(q())
      ctx.globalAlpha = dim ? 0.15 : 1
      ctx.beginPath()
      ctx.arc(n.x ?? 0, n.y ?? 0, r, 0, 2 * Math.PI)
      ctx.fillStyle = colorFor(n.label)
      ctx.fill()
      if (match) {
        ctx.lineWidth = 2 / scale
        ctx.strokeStyle = '#ffd24a'
        ctx.stroke()
      }
      if (n.id === selectedId.value) {
        ctx.lineWidth = 2.5 / scale
        ctx.strokeStyle = '#ffffff'
        ctx.beginPath()
        ctx.arc(n.x ?? 0, n.y ?? 0, r + 2.5 / scale, 0, 2 * Math.PI)
        ctx.stroke()
      }
      if (scale > 1.6 || hoverNodes.has(n.id) || n.id === selectedId.value || match) {
        const fs = Math.max(10 / scale, 3)
        ctx.font = `${fs}px ui-sans-serif, system-ui, sans-serif`
        ctx.fillStyle = dim ? 'rgba(214,216,224,.3)' : '#d6d8e0'
        ctx.textAlign = 'center'
        ctx.fillText(n.name, n.x ?? 0, (n.y ?? 0) + r + fs * 1.1)
      }
      ctx.globalAlpha = 1
    })
    .onNodeHover((node) => {
      hoverNodes.clear()
      hoverLinks.clear()
      if (node) {
        const n = asNode(node)
        hoverNodes.add(n.id)
        adjacency.value.get(n.id)?.forEach((id) => hoverNodes.add(id))
        // These are the same objects force-graph mutates in place, so identity matches what
        // the link accessors receive — only the Set element type needs the cast.
        for (const l of data.value.links) {
          if (linkEnd(l.source) === n.id || linkEnd(l.target) === n.id) {
            hoverLinks.add(l as unknown as LinkObject)
          }
        }
        emit('hover', n)
      } else {
        emit('hover', null)
      }
      if (el.value) el.value.style.cursor = node ? 'pointer' : ''
    })
    .onNodeClick((node) => select(asNode(node)))
    .onBackgroundClick(() => clearSelection())
    .onNodeRightClick(() => graph?.zoomToFit(anim(500), 40))

  graph.d3Force('charge')?.strength(-95)
  graph.d3Force('link')?.distance(38)

  graph.width(window.innerWidth).height(window.innerHeight)
  window.addEventListener('resize', onResize)
  window.setTimeout(() => graph?.zoomToFit(anim(600), 60), SETTLE_MS)
})

function onResize() {
  graph?.width(window.innerWidth).height(window.innerHeight)
}

// Reload → replace the graph data.
watch(data, (d) => graph?.graphData(d as unknown as FgGraphData))
// Hidden labels changed → re-evaluate visibility.
watch(hidden, applyVisibility)
// Selection changed → centre on the node (also nudges a redraw for the selection ring).
watch(selectedId, (id) => centerOn(id))
// Search changed → debounced centre+zoom on the first visible match (avoids zoom-thrash while
// typing; the canvas dimming/match rings react immediately via nodeCanvasObject reading query).
watch(query, () => {
  window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    const term = q()
    if (!term || !graph) return
    const hit = data.value.nodes.find(
      (n) => !hidden.value.has(n.label) && n.name.toLowerCase().includes(term),
    )
    if (hit && hit.x != null && hit.y != null) {
      graph.centerAt(hit.x, hit.y, anim(500))
      graph.zoom(3, anim(500))
    }
  }, 200)
})

onBeforeUnmount(() => {
  window.clearTimeout(searchTimer)
  window.removeEventListener('resize', onResize)
  graph?._destructor()
  graph = null
})
</script>

<template>
  <div
    ref="el"
    class="graph"
    role="img"
    aria-label="Interactive force-directed graph of the homelab Neo4j nodes"
  ></div>
</template>

<style scoped>
.graph {
  position: fixed;
  inset: 0;
}
</style>
