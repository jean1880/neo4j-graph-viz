import { rgbFor } from '../color'
import type { GraphLink, GraphNode, SearchResponse } from '../types'
import { isDrawn, type DrawState } from './state'

/**
 * The GPU-facing representation of the graph.
 *
 * deck.gl renders from typed-array attributes, so the whole node set is a couple of buffer
 * uploads rather than 25 000 draw calls. Everything here exists to keep it that way: nothing in
 * this module runs per frame, only per *change*.
 *
 * Some buffers are fixed at load (`basePositions`, `baseRgb`, `baseRadius`) and some are
 * rewritten whenever interaction state changes (`colour`, `radius`). That split is the whole
 * performance story — a hover rewrites one array in a linear pass, and that is all.
 */
export interface Buffers {
  n: number
  m: number
  /** The graph's own layout, from `/api/graph`. Never mutated — a cleared search restores it.
   *  Three components per node: `z` stays 0 in 2D, so one buffer serves both view modes. */
  basePositions: Float32Array
  /** What is actually drawn: the live layout, mirrored from `node.x/y/z`. */
  positions: Float32Array
  baseRadius: Float32Array
  baseRgb: Uint8Array
  radius: Float32Array
  /** Per-instance sphere scale for 3D — the **relative** size only (a hub is bigger than a leaf),
   *  around 1.0 for a typical node. The overall magnitude rides on `meshSizeScale` instead.
   *
   *  Deliberately split: a binary `getScale` that deck.gl fails to consume falls back to
   *  `[1,1,1]` *silently*, which made sphere size unresponsive to every constant we changed.
   *  With the magnitude in a plain prop, that failure mode can only cost per-degree variation,
   *  never the size itself. */
  scale: Float32Array
  /** Multiplier applied to `scale` by the mesh layer: the world-space size of a typical node. */
  meshSizeScale: number
  /** Relative per-node weight, normalized so a typical node is ~1. Source for `scale`. */
  baseRelScale: Float32Array
  colour: Uint8Array
  linkSource: Float32Array
  linkTarget: Float32Array
  linkColour: Uint8Array
  /** Index of each link's endpoints, so restyle passes never re-resolve ids. */
  linkEnds: Int32Array
  /** `id` → buffer index, so camera moves and labels can read live positions. */
  idIndex: Map<string, number>
  /**
   * Median edge length of the **server's** layout.
   *
   * The simulation scales its forces to this. Measuring it from the live layout instead would
   * compound: each restart would read an already-expanded graph, scale the forces up to match,
   * and expand it further — toggling modes a few times would inflate the graph without bound.
   */
  edgeScale: number
}

const DIM_ALPHA = 38

/**
 * Node radius as a fraction of the layout's median edge length — **separately for 2D and 3D**.
 *
 * This ratio is what decides whether a graph reads as connected or as dust. Node size in absolute
 * units means nothing on its own: the radius that looks right on a compact layout disappears on a
 * spread-out one. Tying it to edge length makes the appearance scale-invariant.
 *
 * The two modes need genuinely different treatment, and it is not a fudge:
 *
 * - **2D** scales the degree weight by a constant, in absolute world units. The 2D layout's own
 *   scale already suits it; deriving size from edge length made nodes balloon.
 * - **3D** ties size to edge length. Nodes fill a volume, span grows with ∛n rather than √n, the
 *   layout is several times sparser relative to its edges, and the camera sees every depth layer
 *   at once — an absolute radius that reads correctly in 2D is invisible dust here.
 *
 * ## Tuning
 *
 * These are the two knobs for node size, and they are independent. Raise to make nodes bigger
 * relative to the space between them, lower to make them smaller. Nothing else needs to change.
 */
const NODE_SCALE_2D = 2.0
const NODE_RADIUS_FRACTION_3D = 0.115

/**
 * Edge colour and opacity, chosen against the dark background by measurement rather than eye.
 *
 * Edges are alpha-blended, so what matters is the **composited** colour, not the base. Against
 * `--bg` (#020617) the old values landed at ~1.1:1 — WCAG 2.1 SC 1.4.11 asks for **3:1** on
 * meaningful non-text graphics, and 1.1:1 is, accurately, invisible. These bases at
 * `LINK_ALPHA_REFERENCE` composite to ~3.0:1.
 *
 * There is a genuine tension here that no single constant resolves: 3:1 *per edge* on a graph
 * with 75 000 of them stacks into a solid sheet, which is less legible, not more. So the
 * reference opacity targets the accessible ratio at ordinary densities, and
 * `linkAlphaFor` steps it down only when sheer edge count forces the trade — at which point the
 * clusters, not the individual lines, are what carry the meaning.
 */
const LINK_BASE = [206, 214, 236] as const
/** 3D loses contrast twice over — edges foreshorten, and they compete with everything behind
 *  them — so the depth view gets a lighter, cooler line. */
const LINK_BASE_3D = [214, 226, 248] as const
/** Composites to ~3.03:1 on #020617. Verified, not estimated. */
const LINK_ALPHA_REFERENCE = 100
/** Edge count at which the reference opacity is used unscaled. */
const LINK_DENSITY_REFERENCE = 4000
/** Never fade below this, however dense: past here the edges stop reading at all. */
const LINK_ALPHA_FLOOR = 34
/** Non-focus edges while something is highlighted — meant to recede, so contrast is not the
 *  goal; the focused neighbourhood is what must be legible. */
const LINK_DIM_ALPHA = 14
/** The focused neighbourhood — the edges the user is actually reading. Nearly opaque, and at
 *  9.1:1 the highest-contrast thing on the canvas, which is where the contrast budget belongs. */
const LINK_FOCUS = [150, 190, 255, 235] as const

/**
 * Edge opacity for a given density.
 *
 * Falls off with the square root of edge count past the reference, because perceived ink
 * coverage grows roughly with the number of overlapping strokes rather than linearly.
 */
export function linkAlphaFor(edgeCount: number): number {
  if (edgeCount <= LINK_DENSITY_REFERENCE) return LINK_ALPHA_REFERENCE
  const scaled = LINK_ALPHA_REFERENCE * Math.sqrt(LINK_DENSITY_REFERENCE / edgeCount)
  return Math.max(LINK_ALPHA_FLOOR, Math.round(scaled))
}

const MATCH_RGB = [255, 210, 74] as const

/** Resolve a link end to a node id. */
type LinkEnd = (end: string | GraphNode) => string

export function buildBuffers(
  nodes: GraphNode[],
  links: GraphLink[],
  linkEnd: LinkEnd,
  dimensions: 2 | 3,
): Buffers {
  const n = nodes.length
  const m = links.length
  const positions = new Float32Array(n * 3)
  const baseRadius = new Float32Array(n)
  const baseRgb = new Uint8Array(n * 3)
  const idIndex = new Map<string, number>()

  for (let i = 0; i < n; i++) {
    const node = nodes[i]
    idIndex.set(node.id, i)
    positions[i * 3] = node.x
    positions[i * 3 + 1] = node.y
    positions[i * 3 + 2] = node.z ?? 0
    const [r, g, b] = rgbFor(node.label)
    baseRgb[i * 3] = r
    baseRgb[i * 3 + 1] = g
    baseRgb[i * 3 + 2] = b
  }

  const linkSource = new Float32Array(m * 3)
  const linkTarget = new Float32Array(m * 3)
  const linkEnds = new Int32Array(m * 2)
  for (let j = 0; j < m; j++) {
    const l = links[j]
    linkEnds[j * 2] = idIndex.get(linkEnd(l.source)) ?? -1
    linkEnds[j * 2 + 1] = idIndex.get(linkEnd(l.target)) ?? -1
  }

  const buf: Buffers = {
    n,
    m,
    edgeScale: 30,
    basePositions: positions,
    positions: Float32Array.from(positions),
    baseRadius,
    baseRgb,
    radius: new Float32Array(n),
    scale: new Float32Array(n * 3),
    meshSizeScale: 1,
    baseRelScale: new Float32Array(n),
    colour: new Uint8Array(n * 4),
    linkSource,
    linkTarget,
    linkColour: new Uint8Array(m * 4),
    linkEnds,
    idIndex,
  }
  syncFromNodes(buf, nodes)
  rescaleNodes(buf, nodes, links, linkEnd, dimensions)
  return buf
}

/**
 * Re-derive node radii from the layout as it currently stands.
 *
 * This has to run **after** the simulation settles, not just at load. The client sim changes the
 * layout's scale — in 3D it expands it several times over — and a radius fixed in world units
 * before that happens shrinks relative to everything around it, which is exactly how a graph
 * ends up looking like scattered dust. Size is a *ratio* to edge length, so it has to be
 * recomputed whenever that length changes.
 *
 * Deliberately does not restart the simulation: `rw` feeds the collision force, so re-settling
 * here would chase its own tail.
 */
export function rescaleNodes(
  buf: Buffers,
  nodes: GraphNode[],
  links: GraphLink[],
  linkEnd: LinkEnd,
  dimensions: 2 | 3,
): void {
  buf.edgeScale = medianEdgeLength(nodes, links, buf.idIndex, linkEnd)

  const weights = nodes.map((node) => node.r).sort((a, b) => a - b)
  const medianWeight = Math.max(weights[Math.floor(weights.length / 2)] ?? 1, 0.001)

  if (dimensions === 2) {
    // The degree weight, scaled — the 2D layout's own scale already suits it.
    buf.meshSizeScale = 1
    for (let i = 0; i < buf.n; i++) {
      nodes[i].rw = nodes[i].r * NODE_SCALE_2D
      buf.baseRadius[i] = nodes[i].rw
      buf.baseRelScale[i] = nodes[i].rw
    }
    return
  }

  // 3D: a typical node is `NODE_RADIUS_FRACTION_3D` of an edge length. The magnitude goes in
  // `meshSizeScale`; `baseRelScale` carries only how much bigger or smaller this node is than
  // typical, so their product is the true world radius in `rw`.
  buf.meshSizeScale = buf.edgeScale * NODE_RADIUS_FRACTION_3D
  for (let i = 0; i < buf.n; i++) {
    buf.baseRelScale[i] = nodes[i].r / medianWeight
    nodes[i].rw = buf.meshSizeScale * buf.baseRelScale[i]
    buf.baseRadius[i] = nodes[i].rw
  }
}

/** Median edge length of a layout — the scale its forces should be in equilibrium with. */
function medianEdgeLength(
  nodes: GraphNode[],
  links: GraphLink[],
  idIndex: Map<string, number>,
  linkEnd: LinkEnd,
): number {
  if (links.length === 0) return 30
  const lengths: number[] = []
  const stride = Math.max(1, Math.floor(links.length / 2000))
  for (let j = 0; j < links.length; j += stride) {
    const s = idIndex.get(linkEnd(links[j].source))
    const t = idIndex.get(linkEnd(links[j].target))
    if (s === undefined || t === undefined) continue
    lengths.push(
      Math.hypot(
        nodes[s].x - nodes[t].x,
        nodes[s].y - nodes[t].y,
        (nodes[s].z ?? 0) - (nodes[t].z ?? 0),
      ),
    )
  }
  if (lengths.length === 0) return 30
  lengths.sort((a, b) => a - b)
  // Median, not mean: a hub's long spokes should not drag the scale up.
  return Math.max(4, lengths[Math.floor(lengths.length / 2)])
}

/** Live position of a node by buffer index. `node.x/y` is the authority — the simulation writes
 *  there — and this buffer is its GPU mirror. */
export function positionAt(buf: Buffers, i: number): [number, number, number] {
  return [buf.positions[i * 3], buf.positions[i * 3 + 1], buf.positions[i * 3 + 2]]
}

/**
 * Mirror `node.x/y` into the GPU buffers. Called once per simulation tick, so it is on the hot
 * path: two linear passes, no allocation.
 */
export function syncFromNodes(buf: Buffers, nodes: GraphNode[]): void {
  const pos = buf.positions
  for (let i = 0; i < buf.n; i++) {
    // `?? 0` is not enough: NaN is neither null nor undefined, and a single NaN coordinate
    // propagates into the bounds and leaves the camera pointing at nothing.
    const { x, y, z } = nodes[i]
    pos[i * 3] = Number.isFinite(x) ? x : 0
    pos[i * 3 + 1] = Number.isFinite(y) ? y : 0
    pos[i * 3 + 2] = Number.isFinite(z) ? (z as number) : 0
  }
  for (let j = 0; j < buf.m; j++) {
    const s = buf.linkEnds[j * 2]
    const t = buf.linkEnds[j * 2 + 1]
    if (s >= 0) {
      buf.linkSource[j * 3] = pos[s * 3]
      buf.linkSource[j * 3 + 1] = pos[s * 3 + 1]
      buf.linkSource[j * 3 + 2] = pos[s * 3 + 2]
    }
    if (t >= 0) {
      buf.linkTarget[j * 3] = pos[t * 3]
      buf.linkTarget[j * 3 + 1] = pos[t * 3 + 1]
      buf.linkTarget[j * 3 + 2] = pos[t * 3 + 2]
    }
  }
}

/**
 * Adopt the positions a search returned, or restore the graph's own layout when it clears.
 *
 * A search re-lays-out the surviving subgraph among itself, so results read as a compact graph
 * rather than as survivors stranded in the holes left by everything removed. The trade is that
 * nodes move: this is not the same picture with pieces taken out.
 */
export function applyPositions(
  buf: Buffers,
  nodes: GraphNode[],
  result: SearchResponse | null,
): void {
  if (!result) {
    for (let i = 0; i < buf.n; i++) {
      nodes[i].x = buf.basePositions[i * 3]
      nodes[i].y = buf.basePositions[i * 3 + 1]
      nodes[i].z = buf.basePositions[i * 3 + 2]
    }
  } else {
    const at = new Map(result.visible.map((v) => [v.id, v]))
    for (let i = 0; i < buf.n; i++) {
      const v = at.get(nodes[i].id)
      // Removed nodes keep their old coordinates; they are drawn at radius 0 either way, and
      // moving them would only churn the buffer.
      if (v) {
        nodes[i].x = v.x
        nodes[i].y = v.y
        nodes[i].z = 0
      }
    }
  }
  syncFromNodes(buf, nodes)
}

/**
 * Rewrite the colour and radius buffers from the current interaction state. One linear pass over
 * the nodes and one over the links — this is the *entire* per-interaction cost of the renderer.
 */
export function restyle(buf: Buffers, state: DrawState): void {
  const { nodes, focusId, focusNodeIds, term, dimensions } = state
  const is3D = dimensions === 3
  const linkAlpha = linkAlphaFor(buf.m)
  const linkBase = is3D ? LINK_BASE_3D : LINK_BASE
  const dimming = focusId !== null

  for (let i = 0; i < buf.n; i++) {
    const node = nodes[i]
    const drawn = isDrawn(state, node)
    // A node that is filtered out gets zero radius rather than zero alpha: an invisible but
    // present point is still pickable, which would let the cursor land on something the user
    // has explicitly removed from view.
    buf.radius[i] = drawn ? buf.baseRadius[i] : 0
    const sc = drawn ? buf.baseRelScale[i] : 0
    buf.scale[i * 3] = sc
    buf.scale[i * 3 + 1] = sc
    buf.scale[i * 3 + 2] = sc

    const o = i * 4
    if (term !== '' && node.nameLower.includes(term)) {
      buf.colour[o] = MATCH_RGB[0]
      buf.colour[o + 1] = MATCH_RGB[1]
      buf.colour[o + 2] = MATCH_RGB[2]
    } else {
      buf.colour[o] = buf.baseRgb[i * 3]
      buf.colour[o + 1] = buf.baseRgb[i * 3 + 1]
      buf.colour[o + 2] = buf.baseRgb[i * 3 + 2]
    }
    buf.colour[o + 3] = !drawn ? 0 : dimming && !focusNodeIds.has(node.id) ? DIM_ALPHA : 255
  }

  const shown = (i: number) => i >= 0 && isDrawn(state, nodes[i])

  for (let j = 0; j < buf.m; j++) {
    const s = buf.linkEnds[j * 2]
    const t = buf.linkEnds[j * 2 + 1]
    const o = j * 4
    if (!shown(s) || !shown(t)) {
      buf.linkColour[o + 3] = 0
      continue
    }
    if (focusId !== null && (nodes[s].id === focusId || nodes[t].id === focusId)) {
      buf.linkColour[o] = LINK_FOCUS[0]
      buf.linkColour[o + 1] = LINK_FOCUS[1]
      buf.linkColour[o + 2] = LINK_FOCUS[2]
      buf.linkColour[o + 3] = LINK_FOCUS[3]
    } else {
      buf.linkColour[o] = linkBase[0]
      buf.linkColour[o + 1] = linkBase[1]
      buf.linkColour[o + 2] = linkBase[2]
      buf.linkColour[o + 3] = dimming ? LINK_DIM_ALPHA : linkAlpha
    }
  }
}
