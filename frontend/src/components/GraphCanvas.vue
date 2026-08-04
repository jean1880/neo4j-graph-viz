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

const {
  data,
  hidden,
  query,
  selectedId,
  hoveredId,
  focusId,
  focusNodeIds,
  focusLinks,
  focusFromHover,
  revealRequest,
  nodeById,
  setHover,
  select,
  clearSelection,
} = useGraph()

// force-graph is imperative and owns its own render loop; we hold the instance and push
// reactive changes into it via watchers. Casts bridge force-graph's generic Node/Link objects
// to our concrete types (its objects carry our extra fields through an index signature).
let graph: ForceGraph | null = null

const asNode = (n: NodeObject) => n as unknown as GraphNode
const asLink = (l: LinkObject) => l as unknown as GraphLink
const nodeVisible = (n: NodeObject) => !hidden.value.has(asNode(n).label)
const linkVisible = (l: LinkObject) =>
  nodeVisible(l.source as NodeObject) && nodeVisible(l.target as NodeObject)
const q = () => query.value.trim().toLowerCase()

/** Drawn node radius. The hit area is painted from the same number — see nodePointerAreaPaint. */
const radiusOf = (n: GraphNode) => 2 + Math.sqrt(n.deg)

// Honour reduced-motion for the discretionary camera moves (the force sim itself is inherent
// to the viz and stays). SETTLE_MS lets the simulation settle before the initial zoom-to-fit.
const anim = (ms: number) =>
  window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : ms
const SETTLE_MS = 700
// How long after a camera gesture settles before we trust force-graph's hit map again. Its
// shadow canvas repaint is throttled, so the first hover reported after a transform lands can
// still be against the old layout; anything it corrects afterwards arrives as a normal event.
const GESTURE_SETTLE_MS = 180
let searchTimer: number | undefined

// --- hover trustworthiness ------------------------------------------------------------------
// force-graph re-runs hit detection every animation frame against the last pointer position, and
// its zoom/pan handler flips `isPointerDragging` for the whole gesture — so every zoom, pan, and
// programmatic camera move emits onNodeHover(null) followed by a burst of hover events against a
// throttled (stale) hit map. Publishing those verbatim is what made a highlight flicker and die
// mid-zoom. During a gesture we freeze the published hover and buffer what force-graph reports,
// then publish the settled value once the camera stops.
let gestureActive = false
let gestureTimer: number | undefined
let pendingHover: string | null = null

function publishHover(id: string | null) {
  pendingHover = id
  if (!gestureActive) setHover(id)
}

function beginGesture() {
  window.clearTimeout(gestureTimer)
  gestureActive = true
}

function endGesture() {
  window.clearTimeout(gestureTimer)
  gestureTimer = window.setTimeout(() => {
    gestureActive = false
    publishHover(pendingHover)
  }, GESTURE_SETTLE_MS)
}

// The user owns the camera from their first wheel/pointer input; after that we never auto-fit.
let userMovedCamera = false
const markUserCamera = () => {
  userMovedCamera = true
}

/**
 * force-graph pauses its render loop once the simulation cools (autoPauseRedraw), and a change
 * to our highlight state alone never marks the canvas dirty — so at rest a new hover, selection,
 * or search match would silently not paint. Re-setting a visibility accessor is force-graph's
 * own redraw notification.
 */
function requestRedraw() {
  graph?.nodeVisibility(nodeVisible)
}

function applyVisibility() {
  graph?.nodeVisibility(nodeVisible).linkVisibility(linkVisible)
}

function centerOn(id: string) {
  const n = nodeById.value.get(id)
  if (!graph || !n || n.x == null || n.y == null) return
  graph.centerAt(n.x, n.y, anim(400))
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
    .nodeVisibility(nodeVisible)
    .linkVisibility(linkVisible)
    .linkColor((l) =>
      focusLinks.value.has(asLink(l)) ? 'rgba(120,160,255,.9)' : 'rgba(150,155,180,.12)',
    )
    .linkWidth((l) => (focusLinks.value.has(asLink(l)) ? 1.6 : 0.5))
    // Particles are a hover-only affordance: they keep force-graph's render loop spinning, so
    // tying them to the (persistent) selection would mean animating forever.
    .linkDirectionalParticles((l) =>
      focusFromHover.value && focusLinks.value.has(asLink(l)) ? 2 : 0,
    )
    .linkDirectionalParticleWidth(2)
    .nodeCanvasObject((node, ctx, scale) => {
      const n = asNode(node)
      const r = radiusOf(n)
      const dim = focusId.value !== null && !focusNodeIds.value.has(n.id)
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
      if (scale > 1.6 || focusNodeIds.value.has(n.id) || match) {
        const fs = Math.max(10 / scale, 3)
        ctx.font = `${fs}px ui-sans-serif, system-ui, sans-serif`
        ctx.fillStyle = dim ? 'rgba(214,216,224,.3)' : '#d6d8e0'
        ctx.textAlign = 'center'
        ctx.fillText(n.name, n.x ?? 0, (n.y ?? 0) + r + fs * 1.1)
      }
      ctx.globalAlpha = 1
    })
    // Without this force-graph hit-tests against its own default circle (sqrt(nodeVal) *
    // nodeRelSize), which does not match the radius we draw: small nodes get a hit area twice
    // their size and big hubs get one smaller than they look. Paint the drawn radius instead
    // (+1px for anti-aliasing), fully opaque so the colour lookup resolves exactly.
    .nodePointerAreaPaint((node, color, ctx) => {
      const n = asNode(node)
      ctx.fillStyle = color
      ctx.beginPath()
      ctx.arc(n.x ?? 0, n.y ?? 0, radiusOf(n) + 1, 0, 2 * Math.PI)
      ctx.fill()
    })
    .onNodeHover((node) => publishHover(node ? asNode(node).id : null))
    // force-graph suppresses hover for the duration of a node drag; keep the dragged node's
    // neighbourhood lit instead of letting it blink out.
    .onNodeDrag((node) => {
      beginGesture()
      pendingHover = asNode(node).id
      setHover(pendingHover)
    })
    .onNodeDragEnd(() => endGesture())
    .onZoom(() => beginGesture())
    .onZoomEnd(() => endGesture())
    .onNodeClick((node) => select(asNode(node)))
    .onBackgroundClick(() => clearSelection())
    .onNodeRightClick(() => graph?.zoomToFit(anim(500), 40))

  graph.d3Force('charge')?.strength(-95)
  graph.d3Force('link')?.distance(38)

  graph.width(window.innerWidth).height(window.innerHeight)
  window.addEventListener('resize', onResize)
  el.value.addEventListener('wheel', markUserCamera, { passive: true })
  el.value.addEventListener('pointerdown', markUserCamera)
  window.setTimeout(() => {
    if (!userMovedCamera) graph?.zoomToFit(anim(600), 60)
  }, SETTLE_MS)
})

function onResize() {
  graph?.width(window.innerWidth).height(window.innerHeight)
}

// Reload → replace the graph data.
watch(data, (d) => graph?.graphData(d as unknown as FgGraphData))
// Hidden labels changed → re-evaluate visibility (this also flags a redraw).
watch(hidden, applyVisibility)
// Any highlight input changed → repaint, since none of these touch force-graph's own state.
watch([focusId, hoveredId, selectedId, query], requestRedraw)
// Only explicit "take me there" intents move the camera; a canvas click does not.
watch(revealRequest, (r) => r && centerOn(r.id))
// Reflect the live hover in the cursor.
watch(hoveredId, (id) => {
  if (el.value) el.value.style.cursor = id ? 'pointer' : ''
})
// Search changed → debounced select+reveal of the first visible match (avoids zoom-thrash while
// typing; the canvas dimming/match rings react immediately via nodeCanvasObject reading query).
// Routing through select() means the detail panel and the highlight follow the search too.
watch(query, () => {
  window.clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => {
    const term = q()
    if (!term || !graph) return
    const hit = data.value.nodes.find(
      (n) => !hidden.value.has(n.label) && n.name.toLowerCase().includes(term),
    )
    if (!hit) return
    select(hit, { reveal: true })
    // Only zoom in if the match would still be lost in the crowd; never yank a user who has
    // already framed the view themselves.
    if (graph.zoom() < 2) graph.zoom(2, anim(500))
  }, 200)
})

onBeforeUnmount(() => {
  window.clearTimeout(searchTimer)
  window.clearTimeout(gestureTimer)
  window.removeEventListener('resize', onResize)
  el.value?.removeEventListener('wheel', markUserCamera)
  el.value?.removeEventListener('pointerdown', markUserCamera)
  graph?._destructor()
  graph = null
})
</script>

<template>
  <div
    ref="el"
    class="graph"
    role="img"
    aria-label="Interactive force-directed graph of the Neo4j nodes"
  ></div>
</template>

<style scoped>
.graph {
  position: fixed;
  inset: 0;
}
</style>
