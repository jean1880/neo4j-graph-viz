import { positionAt, type Buffers } from './buffers'
import { isDrawn, type DrawState } from './state'

/**
 * Camera state for both view modes.
 *
 * `OrthographicView` ignores the rotation fields and `OrbitView` uses them, so one object serves
 * both — which means switching modes does not have to rebuild or reconcile two shapes of state.
 */
export interface ViewState {
  target: [number, number, number]
  zoom: number
  minZoom: number
  maxZoom: number
  /** 3D only: pitch above the horizon, and orbit around the vertical axis. */
  rotationX: number
  rotationOrbit: number
}

export const INITIAL_VIEW: ViewState = {
  target: [0, 0, 0],
  zoom: 0,
  minZoom: -12,
  maxZoom: 12,
  rotationX: 30,
  rotationOrbit: 0,
}

export interface Bounds {
  minX: number
  minY: number
  minZ: number
  maxX: number
  maxY: number
  maxZ: number
}

/**
 * Bounds of everything currently drawn.
 *
 * Read from the live buffer rather than from `node.x/y` so the camera fits what is actually on
 * screen — including a search's re-laid-out survivors, and excluding anything filtered out.
 */
export function visibleBounds(buf: Buffers, state: DrawState): Bounds | null {
  let minX = Infinity
  let minY = Infinity
  let minZ = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  let maxZ = -Infinity
  for (let i = 0; i < buf.n; i++) {
    if (!isDrawn(state, state.nodes[i])) continue
    const [x, y, z] = positionAt(buf, i)
    if (x < minX) minX = x
    if (y < minY) minY = y
    if (z < minZ) minZ = z
    if (x > maxX) maxX = x
    if (y > maxY) maxY = y
    if (z > maxZ) maxZ = z
  }
  return Number.isFinite(minX) ? { minX, minY, minZ, maxX, maxY, maxZ } : null
}

/** A view state framing `bounds` inside a `width × height` viewport. */
export function fitView(
  view: ViewState,
  bounds: Bounds,
  width: number,
  height: number,
  padding = 60,
): ViewState {
  const w = Math.max(width - padding * 2, 1)
  const h = Math.max(height - padding * 2, 1)
  const spanX = Math.max(bounds.maxX - bounds.minX, 1e-6)
  const spanY = Math.max(bounds.maxY - bounds.minY, 1e-6)
  return {
    ...view,
    target: [
      (bounds.minX + bounds.maxX) / 2,
      (bounds.minY + bounds.maxY) / 2,
      (bounds.minZ + bounds.maxZ) / 2,
    ],
    // Depth pushes the far side of the graph away from an orbit camera, so back the zoom off by
    // the z extent as well — otherwise entering 3D crops the graph.
    zoom: Math.log2(Math.min(w / spanX, h / spanY)) - depthAllowance(bounds, spanX, spanY),
  }
}

/** How much extra room a graph's depth needs, in zoom levels. Zero in 2D, where z is flat. */
function depthAllowance(bounds: Bounds, spanX: number, spanY: number): number {
  const spanZ = bounds.maxZ - bounds.minZ
  if (spanZ <= 1e-6) return 0
  return Math.log2(1 + spanZ / Math.max(spanX, spanY))
}

/** A view state centred on a point, leaving the zoom alone. */
export function centreView(view: ViewState, x: number, y: number, z = 0): ViewState {
  return { ...view, target: [x, y, z] }
}

/** The world-space half-extent of the viewport at the current zoom. */
export function worldHalfExtent(
  view: ViewState,
  width: number,
  height: number,
): [number, number] {
  const scale = 2 ** view.zoom
  return [width / 2 / scale, height / 2 / scale]
}
