<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  AmbientLight,
  Deck,
  DirectionalLight,
  LightingEffect,
  OrbitView,
  OrthographicView,
  type PickingInfo,
} from '@deck.gl/core'
import { useGraph } from '../composables/useGraph'
import { useSearch } from '../composables/useSearch'
import { useViewMode } from '../composables/useViewMode'
import {
  applyPositions,
  buildBuffers,
  positionAt,
  rescaleNodes,
  restyle,
  syncFromNodes,
  type Buffers,
} from '../graph/buffers'
import {
  centreView,
  fitView,
  visibleBounds,
  INITIAL_VIEW,
  type ViewState,
} from '../graph/camera'
import { buildLabels, type Label } from '../graph/labels'
import { buildLayers, isNodesLayer, type Ring } from '../graph/layers'
import { createSimulation } from '../graph/simulation'
import { isDrawn, type DrawState } from '../graph/state'
import type { GraphNode } from '../types'

/**
 * Coordinator for the canvas. It owns the deck.gl instance, the camera, and the buffers, and
 * wires the store's reactive state to the modules that do the actual work:
 *
 * - `graph/buffers`    the GPU representation, and every state-driven rewrite of it
 * - `graph/simulation` the warm-started force pass
 * - `graph/camera`     view state and framing
 * - `graph/labels`     the label budget
 * - `graph/layers`     the deck.gl layer stack
 *
 * Nothing here iterates nodes inside a render loop; that is the invariant the whole design
 * protects.
 */
const el = ref<HTMLDivElement | null>(null)

const {
  data,
  hidden,
  selectedId,
  hoveredId,
  focusId,
  focusNodeIds,
  revealRequest,
  nodeById,
  counts,
  setHover,
  select,
  clearSelection,
  linkEnd,
  load,
} = useGraph()

const { query, searchResult, searchVisible, runSearch } = useSearch()
const { dimensions } = useViewMode()

let deck: Deck<OrbitView | OrthographicView> | null = null

/**
 * Orbit in 3D, flat ortho in 2D.
 *
 * Memoized deliberately: deck.gl treats a new View *instance* as a view change and reinitialises
 * the viewport, so constructing one per render leaves nothing on screen. It must stay stable
 * until the mode actually changes.
 */
const deckView = computed(() =>
  dimensions.value === 3 ? new OrbitView({ orbitAxis: 'Y' }) : new OrthographicView({}),
)
let buf: Buffers | null = null
const revision = ref(0)
const view = ref<ViewState>({ ...INITIAL_VIEW })
let userMovedCamera = false

const term = computed(() => query.value.trim().toLowerCase())

/** The one label type still showing, when the legend has narrowed to a single type. */
const isolatedLabel = computed<string | null>(() => {
  const visible = counts.value.filter(([label]) => !hidden.value.has(label))
  return visible.length === 1 ? visible[0][0] : null
})

/** The single description of what should be on screen, handed to every renderer module. */
function drawState(): DrawState {
  return {
    nodes: data.value.nodes,
    hidden: hidden.value,
    surviving: searchVisible.value,
    focusId: focusId.value,
    focusNodeIds: focusNodeIds.value,
    term: term.value,
    dimensions: dimensions.value,
    isolatedLabel: isolatedLabel.value,
  }
}

// --- labels ------------------------------------------------------------------------------------
// Rebuilding labels costs a pass over every node plus a sort, so the result is cached and only
// recomputed when something that changes it changes — never once per simulation tick.
let labelCache: Label[] = []
let labelsDirty = true

function labels(): Label[] {
  if (labelsDirty && buf && el.value) {
    labelCache = buildLabels(
      buf,
      drawState(),
      view.value,
      el.value.clientWidth,
      el.value.clientHeight,
    )
    labelsDirty = false
  }
  return labelCache
}

// --- render ------------------------------------------------------------------------------------
function selectionRing(): Ring[] {
  if (!buf || !selectedId.value) return []
  const node = nodeById.value.get(selectedId.value)
  const i = buf.idIndex.get(selectedId.value)
  if (!node || i === undefined) return []
  return [{ pos: positionAt(buf, i), r: node.rw }]
}

/**
 * Labels, projected to screen coordinates for the DOM overlay.
 *
 * They used to be a deck.gl `TextLayer`. Under `OrbitView`'s perspective projection its pixel
 * sizing is not actually fixed — text shrank as the camera moved in, until only the background
 * plate was left — and the documented cure (clamping `sizeMinPixels`/`sizeMaxPixels`) cost too
 * much performance to keep. Because the label set is budgeted to a couple of hundred, plain DOM
 * is both cheaper and better: native text rendering, genuinely constant size, no SDF artifacts,
 * and it is trivially always on top.
 */
const screenLabels = ref<{ id: string; name: string; x: number; y: number }[]>([])

function projectLabels() {
  const viewport = deck?.getViewports()[0]
  if (!viewport) {
    screenLabels.value = []
    return
  }
  const out: { id: string; name: string; x: number; y: number }[] = []
  for (const l of labels()) {
    const p = viewport.project([l.x, l.y, l.z]) as number[]
    // Depth outside the unit range means the point sits behind the camera.
    if (!Number.isFinite(p[0]) || !Number.isFinite(p[1])) continue
    if (p.length > 2 && (p[2] < 0 || p[2] > 1)) continue
    out.push({ id: l.id, name: l.name, x: p[0], y: p[1] })
  }
  screenLabels.value = out
}

function render() {
  if (!deck || !buf) return
  deck.setProps({
    views: deckView.value,
    // deck.gl types viewState per-view; ours deliberately carries both shapes at once so a mode
    // switch never has to reconcile two objects.
    viewState: view.value as unknown as never,
    layers: buildLayers({
      buf,
      revision: revision.value,
      ring: selectionRing(),
      dimensions: dimensions.value,
      onHover,
      onClick,
    }),
  })
  projectLabels()
}

/** Rewrite the style buffers and repaint. The entire cost of a hover or a legend toggle. */
function restyleAndRender() {
  if (!buf) return
  restyle(buf, drawState())
  revision.value++
  labelsDirty = true
  render()
}

/**
 * Move the camera to the centre of what is drawn, **without touching the zoom.**
 *
 * Used for search results: a search already changes what is on screen and where it sits, and
 * re-zooming on top of that is disorienting — the view lurches instead of simply moving to the
 * answer. Keeping the zoom fixed means the result arrives at the scale the user chose.
 */
function centreOnVisible() {
  if (!buf) return
  const bounds = visibleBounds(buf, drawState())
  if (!bounds) return
  view.value = centreView(
    view.value,
    (bounds.minX + bounds.maxX) / 2,
    (bounds.minY + bounds.maxY) / 2,
    (bounds.minZ + bounds.maxZ) / 2,
  )
  labelsDirty = true
  render()
}

function fitToVisible() {
  if (!buf || !el.value) return
  const bounds = visibleBounds(buf, drawState())
  if (!bounds) return
  view.value = fitView(view.value, bounds, el.value.clientWidth, el.value.clientHeight)
  labelsDirty = true
  render()
}

// --- simulation --------------------------------------------------------------------------------
let tickCount = 0
const sim = createSimulation({
  onTick: () => {
    if (!buf) return
    syncFromNodes(buf, data.value.nodes)
    // Node size is a ratio to edge length, and the settling layout changes that length — so
    // re-derive it **every** tick. Batching it (even every 8) leaves a visible step, because the
    // layout's scale moves fastest in the first few ticks, which is exactly when a batched
    // update lands as one jump. Cheap: one pass over the nodes plus a sampled edge median.
    // Safe mid-flight because the collision radii are frozen (see `simulation.ts`), so this
    // cannot feed back into the layout. No-op in 2D, where size does not depend on edge length.
    rescaleNodes(buf, data.value.nodes, data.value.links, linkEnd, dimensions.value)
    restyle(buf, drawState())
    if (++tickCount % 8 === 0) labelsDirty = true
    revision.value++
    render()
  },
  onEnd: () => {
    if (!buf) return
    // The settled layout is a different scale from the one node sizes were derived from — 3D in
    // particular expands several times over — so re-derive them before framing anything.
    rescaleNodes(buf, data.value.nodes, data.value.links, linkEnd, dimensions.value)
    restyle(buf, drawState())
    revision.value++
    labelsDirty = true
    // The extent changed too, so the framing chosen before the first tick is wrong by now.
    // With a search active, follow it without re-zooming — otherwise the settle would undo the
    // deliberate zoom-preserving recentre a second after it happened.
    if (searchResult.value) centreOnVisible()
    else if (!userMovedCamera) fitToVisible()
    render()
  },
})

/** Settle whatever is currently drawn. Only visible nodes take part — a removed node exerting
 *  force from off-screen would push the layout around for no visible reason. */
function settle() {
  if (!buf) return
  const state = drawState()
  const activeNodes = state.nodes.filter((n) => isDrawn(state, n))
  const active = new Set(activeNodes.map((n) => n.id))
  const activeLinks = data.value.links.filter(
    (l) => active.has(linkEnd(l.source)) && active.has(linkEnd(l.target)),
  )
  sim.start(activeNodes, activeLinks, dimensions.value, buf.edgeScale)
}

function rebuild() {
  buf = buildBuffers(data.value.nodes, data.value.links, linkEnd, dimensions.value)
  restyle(buf, drawState())
  revision.value++
  labelsDirty = true
}

// --- interaction -------------------------------------------------------------------------------
function nodeAt(info: PickingInfo): GraphNode | null {
  if (info.index === undefined || info.index < 0) return null
  if (!isNodesLayer(info.layer?.id)) return null
  return data.value.nodes[info.index] ?? null
}

/**
 * Hover, resolved at the deck level rather than per layer.
 *
 * A layer-level `onHover` only fires while that layer owns the pick, which makes "the pointer
 * left the node" dependent on the layer surviving long enough to report it. Deck-level hover
 * fires on every pointer move with the topmost pick — so moving onto empty background is an
 * ordinary event with no layer, and the highlight clears reliably.
 */
function onHover(info: PickingInfo) {
  const node = nodeAt(info)
  setHover(node ? node.id : null)
  if (el.value) el.value.style.cursor = node ? 'pointer' : ''
}

function onClick(info: PickingInfo) {
  const node = nodeAt(info)
  if (node) select(node)
  else clearSelection()
}

function onLabelClick(id: string) {
  const node = nodeById.value.get(id)
  if (node) select(node)
}

function onLabelHover(id: string | null) {
  setHover(id)
}

function onContextMenu(e: MouseEvent) {
  e.preventDefault()
  fitToVisible()
}

function onResize() {
  labelsDirty = true
  render()
}

// --- lifecycle ---------------------------------------------------------------------------------
onMounted(() => {
  if (!el.value) return
  rebuild()

  deck = new Deck<OrbitView | OrthographicView>({
    parent: el.value,
    views: deckView.value,
    viewState: view.value as unknown as never,
    controller: { doubleClickZoom: false },
    // Spheres need light to read as spheres. Harmless in 2D, where nothing is lit.
    effects: [
      new LightingEffect({
        ambient: new AmbientLight({ color: [255, 255, 255], intensity: 1.4 }),
        dir: new DirectionalLight({
          color: [255, 255, 255],
          intensity: 1.6,
          direction: [-2, -3, -1],
        }),
      }),
    ],
    layers: [],
    onViewStateChange: ({ viewState }) => {
      userMovedCamera = true
      view.value = viewState as unknown as ViewState
      labelsDirty = true
      render()
    },
    // A click that hits no node still has to clear the selection; deck.gl reports that as a
    // pick with no layer rather than a separate background event.
    onClick: (info) => {
      if (!info.layer) clearSelection()
    },
    onHover,
    getCursor: ({ isDragging }) => (isDragging ? 'grabbing' : ''),
  })

  render()
  window.setTimeout(() => {
    if (!userMovedCamera) fitToVisible()
    settle()
  }, 50)

  el.value.addEventListener('contextmenu', onContextMenu)
  window.addEventListener('resize', onResize)
})

// Reload → rebuild every buffer from scratch, refit, and settle again.
watch(data, () => {
  sim.stop()
  rebuild()
  userMovedCamera = false
  render()
  window.setTimeout(() => {
    if (!userMovedCamera) fitToVisible()
    settle()
  }, 50)
})

// Highlight-only changes: one buffer pass, no re-settle. Moving the graph because the cursor
// crossed a node would be intolerable.
watch([focusId, selectedId, hoveredId, term], restyleAndRender)

// Hiding a label changes *which* nodes exert force, so the remainder re-settles — closing the
// gaps left behind is exactly what the simulation should do.
watch(hidden, () => {
  restyleAndRender()
  settle()
})

// A search changes both what is drawn and where: survivors come back laid out among themselves,
// so the camera refits and the subgraph settles.
watch(searchResult, (r) => {
  if (!buf) return
  sim.stop()
  applyPositions(buf, data.value.nodes, r)
  restyleAndRender()
  // Recentre, but keep the zoom the user is at. Clearing the search returns to the whole graph,
  // where a full refit is what you want — a preserved zoom would leave you inside a fragment.
  if (r) centreOnVisible()
  else fitToVisible()
  settle()
  // Follow the top hit, so the detail panel and highlight track the search.
  const top = r?.matches[0]
  if (top) {
    const node = nodeById.value.get(top.id)
    if (node) select(node)
  }
})

// Switching dimension re-runs the simulation in the new number of dimensions — a flat layout
// has no depth to reveal, so this is a re-settle rather than a camera change. The camera refits
// once it has something with actual extent to frame.
watch(dimensions, () => {
  sim.stop()
  // Refetch rather than re-settle in place: the backend computes a real octree layout for 3D,
  // which is a far better starting point than inflating a flat one. The `data` watcher below
  // rebuilds the buffers, refits, and settles once it lands.
  void load()
})

// Search → debounced backend query. Debounced because it is a network call that also costs the
// server a layout pass; there is no point issuing one per keystroke.
let searchTimer: number | undefined
watch(term, (t) => {
  window.clearTimeout(searchTimer)
  if (t === '') {
    void runSearch('')
    return
  }
  searchTimer = window.setTimeout(() => void runSearch(t), 250)
})

watch(revealRequest, (r) => {
  if (!r || !buf) return
  const i = buf.idIndex.get(r.id)
  if (i === undefined) return
  const [x, y] = positionAt(buf, i)
  view.value = centreView(view.value, x, y)
  labelsDirty = true
  render()
})

onBeforeUnmount(() => {
  sim.stop()
  window.clearTimeout(searchTimer)
  window.removeEventListener('resize', onResize)
  el.value?.removeEventListener('contextmenu', onContextMenu)
  deck?.finalize()
  deck = null
  buf = null
})
</script>

<template>
  <div
    ref="el"
    class="graph"
    role="img"
    aria-label="Interactive force-directed graph of the Neo4j nodes"
  ></div>
  <!-- Labels as DOM rather than canvas: constant size at any zoom, crisp at any DPI, and
       always over the graph. The container ignores pointer events so it never steals a drag
       from the canvas; the labels themselves accept them, so clicking a name selects its node. -->
  <div class="labels" aria-hidden="true">
    <button
      v-for="l in screenLabels"
      :key="l.id"
      type="button"
      class="label"
      :style="{ transform: `translate3d(${l.x}px, ${l.y}px, 0)` }"
      @click.stop="onLabelClick(l.id)"
      @mouseenter="onLabelHover(l.id)"
      @mouseleave="onLabelHover(null)"
    >
      {{ l.name }}
    </button>
  </div>
</template>

<style scoped>
.labels {
  position: fixed;
  inset: 0;
  z-index: 5;
  /* The overlay must never intercept a pan or a zoom — only the labels themselves are live. */
  pointer-events: none;
  overflow: hidden;
}
.label {
  position: absolute;
  top: 0;
  left: 0;
  /* translate3d positions it; this centres it on the node and lifts it clear. */
  margin: -28px 0 0 0;
  translate: -50% 0;
  pointer-events: auto;
  font: inherit;
  font-size: var(--text-sm);
  line-height: 1.4;
  white-space: nowrap;
  padding: 3px 8px;
  min-height: 24px;
  border: none;
  border-radius: var(--radius-sm, 4px);
  background: color-mix(in srgb, var(--bg, #020617) 88%, transparent);
  color: var(--text, #e8ebf4);
  cursor: pointer;
}
.label:hover {
  background: var(--bg, #020617);
  color: #fff;
}
.graph {
  position: fixed;
  inset: 0;
  background: var(--bg, #020617);
}
</style>
