<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import {
  AmbientLight,
  Deck,
  DirectionalLight,
  LightingEffect,
  OrbitView,
  OrthographicView,
  type PickingInfo,
} from "@deck.gl/core";
import { useGraph } from "../composables/useGraph";
import { useLayoutSettings } from "../composables/useLayoutSettings";
import { useSearch } from "../composables/useSearch";
import { useViewMode } from "../composables/useViewMode";
import {
  applyPositions,
  buildBuffers,
  positionAt,
  rescaleNodes,
  restyle,
  syncFromNodes,
  type Buffers,
} from "../graph/buffers";
import {
  centreView,
  easeInOutCubic,
  fitView,
  lerpView,
  viewsClose,
  visibleBounds,
  INITIAL_VIEW,
  type ViewState,
} from "../graph/camera";
import { buildLabels, type Label } from "../graph/labels";
import { buildLayers, isNodesLayer, type Ring } from "../graph/layers";
import { createSimulation } from "../graph/simulation";
import { isDrawn, type DrawState } from "../graph/state";
import type { GraphNode } from "../types";

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
const el = ref<HTMLDivElement | null>(null);

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
} = useGraph();

const { query, searchResult, searchVisible, runSearch } = useSearch();
const { dimensions } = useViewMode();
const { settings } = useLayoutSettings();

let deck: Deck<OrbitView | OrthographicView> | null = null;

/**
 * Orbit in 3D, flat ortho in 2D.
 *
 * Memoized deliberately: deck.gl treats a new View *instance* as a view change and reinitialises
 * the viewport, so constructing one per render leaves nothing on screen. It must stay stable
 * until the mode actually changes.
 */
const deckView = computed(() =>
  dimensions.value === 3
    ? new OrbitView({ orbitAxis: "Y" })
    : new OrthographicView({}),
);
let buf: Buffers | null = null;
const revision = ref(0);
const view = ref<ViewState>({ ...INITIAL_VIEW });
let userMovedCamera = false;

const term = computed(() => query.value.trim().toLowerCase());

/** The one label type still showing, when the legend has narrowed to a single type. */
const isolatedLabel = computed<string | null>(() => {
  const visible = counts.value.filter(([label]) => !hidden.value.has(label));
  return visible.length === 1 ? visible[0][0] : null;
});

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
  };
}

// --- labels ------------------------------------------------------------------------------------
// Rebuilding labels costs a pass over every node plus a sort, so the result is cached and only
// recomputed when something that changes it changes — never once per simulation tick.
let labelCache: Label[] = [];
let labelsDirty = true;

function labels(): Label[] {
  if (labelsDirty && buf && el.value) {
    labelCache = buildLabels(
      buf,
      drawState(),
      view.value,
      el.value.clientWidth,
      el.value.clientHeight,
    );
    labelsDirty = false;
  }
  return labelCache;
}

// --- render ------------------------------------------------------------------------------------
function selectionRing(): Ring[] {
  if (!buf || !selectedId.value) return [];
  const node = nodeById.value.get(selectedId.value);
  const i = buf.idIndex.get(selectedId.value);
  if (!node || i === undefined) return [];
  return [{ pos: positionAt(buf, i), r: node.rw }];
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
const screenLabels = ref<{ id: string; name: string; x: number; y: number }[]>(
  [],
);

/**
 * Whether deck has finished initialising and has a viewport to project against.
 *
 * `getViewports()` **asserts** rather than returning empty before deck has sized itself against
 * the canvas, and the first scheduled frame can land in exactly that window — which is where the
 * `deck.gl: assertion failed` on every load came from. deck's own `onLoad` is the authoritative
 * signal; a try/catch would swallow the symptom while still paying the throw.
 */
let deckReady = false;

function projectLabels() {
  if (!deckReady) {
    screenLabels.value = [];
    return;
  }
  const viewport = deck?.getViewports()[0];
  if (!viewport) {
    screenLabels.value = [];
    return;
  }
  const out: { id: string; name: string; x: number; y: number }[] = [];
  for (const l of labels()) {
    const p = viewport.project([l.x, l.y, l.z]) as number[];
    // Depth outside the unit range means the point sits behind the camera.
    if (!Number.isFinite(p[0]) || !Number.isFinite(p[1])) continue;
    if (p.length > 2 && (p[2] < 0 || p[2] > 1)) continue;
    out.push({ id: l.id, name: l.name, x: p[0], y: p[1] });
  }
  screenLabels.value = out;
}

/**
 * The frame scheduler.
 *
 * Nothing in this component paints synchronously. Every input that changes the picture — a
 * simulation tick, a hover, a wheel event, a slider, a resize — marks what it invalidated and
 * asks for a frame; `paint` runs **once** per animation frame and does whatever the accumulated
 * flags require.
 *
 * This matters because the events do not arrive at frame rate. A trackpad emits wheel events
 * faster than the display refreshes, deck.gl reports a hover per pointer move, and a legend
 * toggle plus a search plus a settle can all land between two frames. Painting per event meant
 * `setProps` — and, worse, the O(n) `restyle` pass behind it — ran several times to produce one
 * visible image. Coalescing makes that cost per *frame* instead of per *event*, which is the
 * whole point of the typed-array design: the buffers are cheap to rewrite once, not five times.
 *
 * The flags are deliberately separate. A camera move needs no restyle; a hover needs no
 * reprojection of a label set that has not moved. Merging them would hand back the savings.
 */
let frame = 0;
let needsStyle = false;
let needsSync = false;

/**
 * An in-flight camera move.
 *
 * The refit after a settle used to be applied as a single assignment, which is what made it ugly:
 * the graph was in one place and then, one frame later, somewhere else. The framing itself was
 * never the problem — the discontinuity was. Animating the same move over a few hundred
 * milliseconds keeps the viewer's sense of where things are.
 *
 * Driven by our own scheduler rather than deck.gl's `transitionDuration`, because `paint` writes
 * `viewState` on **every** frame: deck treats each of those writes as a new target and restarts
 * (or cancels) its own transition, so its interpolators cannot survive this render loop.
 */
interface CameraTween {
  from: ViewState;
  to: ViewState;
  start: number;
  duration: number;
}
let tween: CameraTween | null = null;

const TWEEN_MS = 550;

/**
 * Animate the camera to `to`, unless the user has taken hold of it.
 *
 * Cancelled by any real interaction — chasing a camera the user is actively driving is worse than
 * never moving it at all.
 */
function animateTo(to: ViewState, duration = TWEEN_MS) {
  if (viewsClose(view.value, to)) {
    view.value = to;
    labelsDirty = true;
    render();
    return;
  }
  tween = { from: view.value, to, start: performance.now(), duration };
  labelsDirty = true;
  render();
}

function cancelTween() {
  tween = null;
}

/** Advance an in-flight camera move. Returns true while the tween still owes frames. */
function stepTween(now: number): boolean {
  if (!tween) return false;
  const t = Math.min(1, (now - tween.start) / tween.duration);
  view.value = lerpView(tween.from, tween.to, easeInOutCubic(t));
  labelsDirty = true;
  if (t >= 1) {
    tween = null;
    return false;
  }
  return true;
}

/** Ask for a frame. Idempotent — repeated calls within one frame collapse into a single paint. */
function render() {
  if (frame) return;
  frame = requestAnimationFrame(paint);
}

/** Mark the style buffers stale and ask for a frame. The entire cost of a hover or a legend
 *  toggle at event time; the buffer pass itself happens in `paint`. */
function restyleAndRender() {
  needsStyle = true;
  render();
}

function paint() {
  frame = 0;
  if (!deck || !buf) return;
  // Ask for the next frame *before* drawing this one, so a camera move keeps its own cadence
  // whether or not anything else is invalidating the scene.
  if (stepTween(performance.now())) render();
  if (needsSync) {
    needsSync = false;
    syncAndRescale();
  }
  if (needsStyle) {
    needsStyle = false;
    restyle(buf, drawState());
    revision.value++;
    labelsDirty = true;
  }
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
      linkThickness: settings.value.linkThickness,
      nodeSize: settings.value.nodeSize,
      onHover,
      onClick,
    }),
  });
  projectLabels();
}

/**
 * Move the camera to the centre of what is drawn, **without touching the zoom.**
 *
 * Used for search results: a search already changes what is on screen and where it sits, and
 * re-zooming on top of that is disorienting — the view lurches instead of simply moving to the
 * answer. Keeping the zoom fixed means the result arrives at the scale the user chose.
 */
function centreOnVisible() {
  if (!buf) return;
  const bounds = visibleBounds(buf, drawState());
  if (!bounds) return;
  animateTo(
    centreView(
      view.value,
      (bounds.minX + bounds.maxX) / 2,
      (bounds.minY + bounds.maxY) / 2,
      (bounds.minZ + bounds.maxZ) / 2,
    ),
  );
}

function fitToVisible({ animate = true } = {}) {
  if (!buf || !el.value) return;
  const bounds = visibleBounds(buf, drawState());
  if (!bounds) return;
  const target = fitView(
    view.value,
    bounds,
    el.value.clientWidth,
    el.value.clientHeight,
  );
  // Snapping is for framings whose starting pose describes different content (first load, reload,
  // mode switch); animating is for corrections to a scene already on screen.
  if (!animate) {
    cancelTween();
    view.value = target;
    labelsDirty = true;
    render();
    return;
  }
  animateTo(target);
}

// --- simulation --------------------------------------------------------------------------------
let tickCount = 0;

/**
 * Mirror the layout into the GPU buffers and re-derive node sizes from it.
 *
 * Node size is a ratio to edge length, and the settling layout changes that length — so it is
 * re-derived on **every** frame the layout moved. Batching it further leaves a visible step,
 * because the layout's scale moves fastest in the first few frames, which is exactly when a
 * batched update lands as one jump. Cheap: two linear passes plus a sampled edge median. Safe
 * mid-flight because the collision radii are frozen (see `simulation.ts`), so it cannot feed back
 * into the layout. Effectively a no-op in 2D, where size does not depend on edge length.
 */
function syncAndRescale() {
  if (!buf) return;
  syncFromNodes(buf, data.value.nodes);
  rescaleNodes(
    buf,
    data.value.nodes,
    data.value.links,
    linkEnd,
    dimensions.value,
    settings.value.nodeSize,
  );
}

const sim = createSimulation(
  {
    // A tick moves nodes; it does not paint. The worker posts them faster than the display
    // refreshes — mirroring positions per message would copy 60 000 of them into buffers no one is
    // going to look at. The flag makes the last message before a frame the only one that costs
    // anything.
    onTick: () => {
      needsSync = true;
      needsStyle = true;
      if (++tickCount % 8 === 0) labelsDirty = true;
      render();
    },
    onEnd: () => {
      if (!buf) return;
      // The settled layout is a different scale from the one node sizes were derived from — 3D in
      // particular expands several times over — so re-derive them before framing anything. Done
      // synchronously rather than deferred, because the camera decision below reads the bounds this
      // produces and must not see the previous frame's.
      syncAndRescale();
      needsStyle = true;
      labelsDirty = true;
      // The extent has changed — often by a lot, since a settle at 60k expands the layout well
      // past its starting size — so the framing chosen before the first tick is now wrong.
      //
      // This correction is **animated**, and that distinction is the whole point. Applied as a
      // single assignment it read as the view lurching on its own a second after load; travelled
      // over half a second it reads as the camera following the graph, and the viewer keeps track
      // of where everything went. A camera the user has already taken hold of is left alone.
      //
      // A search only recentres, never re-zooms: it has already changed what is on screen, and
      // changing the scale on top of that is one change too many.
      if (searchResult.value) centreOnVisible();
      else if (!userMovedCamera) fitToVisible();
      render();
    },
  },
  linkEnd,
);

/** Settle whatever is currently drawn. Only visible nodes take part — a removed node exerting
 *  force from off-screen would push the layout around for no visible reason. */
function settle() {
  if (!buf) return;
  const state = drawState();
  const activeNodes = state.nodes.filter((n) => isDrawn(state, n));
  const active = new Set(activeNodes.map((n) => n.id));
  const activeLinks = data.value.links.filter(
    (l) => active.has(linkEnd(l.source)) && active.has(linkEnd(l.target)),
  );
  // `baseEdgeScale`, never `edgeScale`: the forces must be scaled to the layout the server
  // produced, not to the one they themselves have already expanded.
  sim.start(
    activeNodes,
    activeLinks,
    dimensions.value,
    buf.baseEdgeScale,
    settings.value,
  );
}

function rebuild() {
  buf = buildBuffers(
    data.value.nodes,
    data.value.links,
    linkEnd,
    dimensions.value,
    settings.value.nodeSize,
  );
  restyle(buf, drawState());
  revision.value++;
  labelsDirty = true;
}

// --- interaction -------------------------------------------------------------------------------
function nodeAt(info: PickingInfo): GraphNode | null {
  if (info.index === undefined || info.index < 0) return null;
  if (!isNodesLayer(info.layer?.id)) return null;
  return data.value.nodes[info.index] ?? null;
}

/**
 * Hover, resolved at the deck level rather than per layer.
 *
 * A layer-level `onHover` only fires while that layer owns the pick, which makes "the pointer
 * left the node" dependent on the layer surviving long enough to report it. Deck-level hover
 * fires on every pointer move with the topmost pick — so moving onto empty background is an
 * ordinary event with no layer, and the highlight clears reliably.
 */
let cursor = "";
function onHover(info: PickingInfo) {
  const node = nodeAt(info);
  setHover(node ? node.id : null);
  // Written only when it actually changes. This fires on every pointer move, and assigning an
  // inline style is a synchronous DOM write — cheap once, but not something to do a hundred
  // times a second to set it to the value it already had.
  const want = node ? "pointer" : "";
  if (el.value && want !== cursor) {
    cursor = want;
    el.value.style.cursor = want;
  }
}

function onClick(info: PickingInfo) {
  const node = nodeAt(info);
  if (node) select(node);
  else clearSelection();
}

function onLabelClick(id: string) {
  const node = nodeById.value.get(id);
  if (node) select(node);
}

function onLabelHover(id: string | null) {
  setHover(id);
}

function onContextMenu(e: MouseEvent) {
  e.preventDefault();
  fitToVisible();
}

function onResize() {
  labelsDirty = true;
  render();
}

// --- lifecycle ---------------------------------------------------------------------------------
onMounted(() => {
  if (!el.value) return;
  rebuild();

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
    onViewStateChange: ({ viewState, interactionState }) => {
      // **Only a real gesture counts as the user taking the camera.**
      //
      // deck emits this callback for its own reasons too — notably when the View *class* changes,
      // which is exactly what a 2D→3D switch does. Treating that as user input latched
      // `userMovedCamera` right after the reload had cleared it, so the post-load fit was skipped
      // and the camera stayed at the old 2D zoom while the 3D layout sat off-screen: switching to
      // 3D after zooming showed a blank canvas. `interactionState` is what separates a gesture
      // from bookkeeping.
      const gesture =
        interactionState?.isDragging ||
        interactionState?.isPanning ||
        interactionState?.isZooming ||
        interactionState?.isRotating;
      if (gesture) {
        // Genuine input outranks a camera move in flight.
        cancelTween();
        userMovedCamera = true;
      }
      view.value = viewState as unknown as ViewState;
      labelsDirty = true;
      render();
    },
    // A click that hits no node still has to clear the selection; deck.gl reports that as a
    // pick with no layer rather than a separate background event.
    onClick: (info) => {
      if (!info.layer) clearSelection();
    },
    onHover,
    getCursor: ({ isDragging }) => (isDragging ? "grabbing" : ""),
    // deck is only safe to project against once it reports itself loaded; until then the label
    // pass is skipped rather than allowed to assert.
    onLoad: () => {
      deckReady = true;
      labelsDirty = true;
      render();
    },
  });

  render();
  window.setTimeout(() => {
    if (!userMovedCamera) fitToVisible({ animate: false });
    settle();
  }, 50);

  el.value.addEventListener("contextmenu", onContextMenu);
  window.addEventListener("resize", onResize);
});

// Reload → rebuild every buffer from scratch, refit, and settle again.
//
// **This framing snaps, and must.** A camera move is only meaningful when its start and end frame
// the *same* content. This path replaces the content wholesale — it is also how a 2D/3D switch
// arrives, since changing dimensionality refetches a layout computed by a different algorithm, in
// different coordinates, at a different scale. The old pose describes a graph that no longer
// exists, so interpolating from it flies the camera through empty space: animating here showed a
// blank canvas mid-transition. The eye accepts an instant reframe when the scene changed with it.
//
// The correction *after* the settle is the one that animates — there the content is the same and
// only its extent has grown.
watch(data, () => {
  sim.stop();
  rebuild();
  userMovedCamera = false;
  render();
  window.setTimeout(() => {
    if (!userMovedCamera) fitToVisible({ animate: false });
    settle();
  }, 50);
});

// Highlight-only changes: one buffer pass, no re-settle. Moving the graph because the cursor
// crossed a node would be intolerable.
watch([focusId, selectedId, hoveredId, term], restyleAndRender);

// Hiding a label changes *which* nodes exert force, so the remainder re-settles — closing the
// gaps left behind is exactly what the simulation should do.
watch(hidden, () => {
  restyleAndRender();
  settle();
});

// --- settings ----------------------------------------------------------------------------------
// Only one slider is purely cosmetic. Link thickness changes a stroke width and nothing else, so
// it repaints and stops there.
watch(() => settings.value.linkThickness, render);

/**
 * Every other slider re-lays-out, in two phases.
 *
 * **Immediately:** re-derive the radii and repaint, so dragging node size feels like a size
 * control rather than a form you submit.
 *
 * **Debounced:** restart the simulation. This is what makes bigger nodes actually push each
 * other apart — the collision force reads `rw`, and it *snapshots* those radii at start (a live
 * read would close a radius → spacing → radius feedback loop; see the note in `simulation.ts`).
 * A frozen snapshot means growing a node only separates its neighbours on the next start, so
 * without this restart the nodes would simply overlap more. Rescaling before the debounce fires
 * is what puts the new radii into that snapshot.
 *
 * Restarting from the live positions rather than the server's is deliberate: dragging spacing up
 * should expand the layout you are looking at, not throw it away and start over.
 *
 * Debounced because a dragged slider emits a value per pixel of travel, and a simulation restart
 * per pixel would be unusable.
 */
let relayoutTimer: number | undefined;
watch(
  () => [
    settings.value.nodeSize,
    settings.value.linkDistance,
    settings.value.repel,
    settings.value.linkForce,
    settings.value.centreForce,
  ],
  () => {
    if (!buf) return;
    rescaleNodes(
      buf,
      data.value.nodes,
      data.value.links,
      linkEnd,
      dimensions.value,
      settings.value.nodeSize,
    );
    restyleAndRender();
    window.clearTimeout(relayoutTimer);
    relayoutTimer = window.setTimeout(settle, 160);
  },
);

// A search changes both what is drawn and where: survivors come back laid out among themselves,
// so the camera refits and the subgraph settles.
watch(searchResult, (r) => {
  if (!buf) return;
  sim.stop();
  applyPositions(buf, data.value.nodes, data.value.links, linkEnd, r);
  restyleAndRender();
  // Recentre, but keep the zoom the user is at. Clearing the search returns to the whole graph,
  // where a full refit is what you want — a preserved zoom would leave you inside a fragment.
  if (r) centreOnVisible();
  else fitToVisible();
  settle();
  // Follow the top hit, so the detail panel and highlight track the search.
  const top = r?.matches[0];
  if (top) {
    const node = nodeById.value.get(top.id);
    if (node) select(node);
  }
});

// Switching dimension re-runs the simulation in the new number of dimensions — a flat layout
// has no depth to reveal, so this is a re-settle rather than a camera change. The camera refits
// once it has something with actual extent to frame.
watch(dimensions, () => {
  sim.stop();
  // **Reset the camera, do not carry it across.**
  //
  // An orthographic `zoom` and an orbit `zoom` are not the same quantity. 2D clamps at
  // `maxZoom: 12`, and handing a scale of 2^12 to `OrbitView` collapses its near/far planes into a
  // degenerate projection — deck reports `Pixel project matrix not invertible` and draws nothing.
  // Zoomed in far enough, switching to 3D therefore produced a blank canvas.
  //
  // There is nothing to preserve anyway: the two modes are different layouts in different
  // coordinates (see CLAUDE.md), so a pose from one says nothing about the other. Starting neutral
  // also gives the post-load fit a sane camera to frame from.
  cancelTween();
  view.value = { ...INITIAL_VIEW };
  userMovedCamera = false;
  // Refetch rather than re-settle in place: the backend computes a real octree layout for 3D,
  // which is a far better starting point than inflating a flat one. The `data` watcher below
  // rebuilds the buffers, refits, and settles once it lands.
  void load();
});

// Search → debounced backend query. Debounced because it is a network call that also costs the
// server a layout pass; there is no point issuing one per keystroke.
let searchTimer: number | undefined;
watch(term, (t) => {
  window.clearTimeout(searchTimer);
  if (t === "") {
    void runSearch("");
    return;
  }
  searchTimer = window.setTimeout(() => void runSearch(t), 250);
});

watch(revealRequest, (r) => {
  if (!r || !buf) return;
  const i = buf.idIndex.get(r.id);
  if (i === undefined) return;
  const [x, y] = positionAt(buf, i);
  view.value = centreView(view.value, x, y);
  labelsDirty = true;
  render();
});

onBeforeUnmount(() => {
  // Terminate, not just stop: a worker outlives the component that owns it.
  sim.dispose();
  window.clearTimeout(searchTimer);
  window.clearTimeout(relayoutTimer);
  // A frame already requested would otherwise fire after `deck` is finalized.
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  cancelTween();
  window.removeEventListener("resize", onResize);
  el.value?.removeEventListener("contextmenu", onContextMenu);
  deck?.finalize();
  deck = null;
  buf = null;
});
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
