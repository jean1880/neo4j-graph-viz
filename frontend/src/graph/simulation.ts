import type { GraphLink, GraphNode } from '../types'
import { DEFAULT_SETTINGS, type LayoutSettings } from './settings'

/**
 * The client-side polish pass over the server's layout — **driven from a Web Worker.**
 *
 * The server gets the *structure* right: it can afford a Barnes–Hut pass across every core that
 * the browser never could. But at θ=0.9 it is an approximation, and nodes end up overlapping.
 * This finishes the job with real charge, link, and collision forces, warm-started from an
 * already-good configuration so it converges in ~100 ticks rather than the ~15 seconds a cold
 * start took.
 *
 * ## Why it is off-thread
 *
 * Measured at 60 000 nodes: **423 ms per tick**, ~100 ticks, all of it blocking. The page ran at
 * 2.4 fps and answered no input for 30 seconds (`docs/perf-60k.md`). Rendering was never the
 * constraint — a hover storm at the same node count did not move frame time at all. So the tick
 * moved to a worker, and the main thread's share of a settle became: copy a `Float32Array`, draw.
 *
 * The transport is deliberately primitive — positions and index pairs, no ids, no objects. The
 * worker posts each tick's positions as a **transferred** buffer, so a 60k update is a pointer
 * hand-off rather than a structured clone.
 *
 * ## What the caller still owns
 *
 * `GraphNode.x/y/z` remains the authority the rest of the app reads (bounds, labels, node sizing),
 * so every message writes back into those objects. That write is O(n) and measured in a
 * millisecond at 60k — the part that was expensive is the part that left.
 */

export interface SimulationCallbacks {
  onTick: () => void
  onEnd: () => void
}

export interface SimulationHandle {
  /** Start (or restart) over the given nodes and links, in 2 or 3 dimensions. Fewer than two
   *  nodes is a no-op. */
  start: (
    nodes: GraphNode[],
    links: GraphLink[],
    dimensions: 2 | 3,
    /** Length scale to hold the forces in equilibrium with — the server layout's median edge. */
    dist: number,
    /** The user's force sliders for this mode. */
    settings?: LayoutSettings,
  ) => void
  stop: () => void
  /** Release the worker. After this the handle is dead. */
  dispose: () => void
  readonly running: boolean
}

type WorkerMessage = { type: 'tick' | 'end'; positions: Float32Array }

/** Resolve a link end to a node id — links may carry either the id or the node itself. */
type LinkEnd = (end: string | GraphNode) => string

export function createSimulation(
  cb: SimulationCallbacks,
  linkEnd: LinkEnd,
): SimulationHandle {
  const worker = new Worker(new URL('./layout.worker.ts', import.meta.url), { type: 'module' })
  let running = false
  /** The node array the in-flight run belongs to. A late message from a superseded run would
   *  otherwise write positions into whatever graph is loaded now. */
  let active: GraphNode[] | null = null

  worker.onmessage = (e: MessageEvent<WorkerMessage>) => {
    const { type, positions } = e.data
    const nodes = active
    if (!nodes) return
    const n = Math.min(nodes.length, positions.length / 3)
    for (let i = 0; i < n; i++) {
      nodes[i].x = positions[i * 3]
      nodes[i].y = positions[i * 3 + 1]
      nodes[i].z = positions[i * 3 + 2]
    }
    if (type === 'end') {
      running = false
      active = null
      cb.onEnd()
    } else {
      cb.onTick()
    }
  }

  function stop() {
    if (!running) return
    worker.postMessage({ type: 'stop' })
    running = false
    active = null
  }

  function start(
    nodes: GraphNode[],
    links: GraphLink[],
    dimensions: 2 | 3 = 2,
    dist = 30,
    settings: LayoutSettings = DEFAULT_SETTINGS[dimensions],
  ) {
    stop()
    if (nodes.length < 2) return

    const index = new Map<string, number>()
    const positions = new Float32Array(nodes.length * 3)
    const radii = new Float32Array(nodes.length)
    for (let i = 0; i < nodes.length; i++) {
      const node = nodes[i]
      index.set(node.id, i)
      positions[i * 3] = node.x
      positions[i * 3 + 1] = node.y
      positions[i * 3 + 2] = dimensions === 3 ? (node.z ?? 0) : 0
      // Snapshotted here rather than read live in the worker: see the note on the collide force.
      radii[i] = node.rw * 1.1
    }

    // Index pairs, so no string crosses the boundary and the worker needs no id map.
    const pairs = new Int32Array(links.length * 2)
    let m = 0
    for (const l of links) {
      const s = index.get(linkEnd(l.source))
      const t = index.get(linkEnd(l.target))
      if (s === undefined || t === undefined) continue
      pairs[m * 2] = s
      pairs[m * 2 + 1] = t
      m++
    }
    const links2 = pairs.subarray(0, m * 2)

    active = nodes
    running = true

    // Copied field by field, **not** passed through. `settings` arrives as a Vue reactive Proxy,
    // and `postMessage` cannot structure-clone a Proxy: it throws `DataCloneError`, the message
    // is never delivered, and the settle silently does not happen. The page looks fine — it is
    // simply showing the server's layout forever — which is exactly the kind of failure a
    // frame-time measurement reports as a triumph.
    const plain: LayoutSettings = {
      nodeSize: settings.nodeSize,
      linkThickness: settings.linkThickness,
      linkDistance: settings.linkDistance,
      repel: settings.repel,
      linkForce: settings.linkForce,
      centreForce: settings.centreForce,
    }

    // Reduced motion: converge without animating it. The ticks still run — all at once, in the
    // worker — so the user is handed a settled graph instead of watching it move.
    const immediate = window.matchMedia('(prefers-reduced-motion: reduce)').matches

    worker.postMessage(
      {
        type: 'start',
        positions,
        radii,
        links: links2,
        dimensions,
        dist,
        settings: plain,
        immediate,
      },
      // `links2` may be a view onto `pairs`; transfer the backing buffer once.
      [positions.buffer, radii.buffer, pairs.buffer],
    )
  }

  return {
    start,
    stop,
    dispose: () => {
      running = false
      active = null
      worker.terminate()
    },
    get running() {
      return running
    },
  }
}
