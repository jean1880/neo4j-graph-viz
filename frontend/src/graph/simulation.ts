import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  forceZ,
  type Simulation,
} from 'd3-force-3d'
import type { GraphLink, GraphNode } from '../types'

/**
 * A full force simulation, warm-started from the server's layout.
 *
 * The server gets the *structure* right — it can afford a Barnes–Hut pass across every core that
 * the browser never could. But at θ=0.9 it is an approximation, and nodes end up overlapping.
 * This finishes the job with real charge, link, and collision forces, run against an already-good
 * configuration so it converges in ~100 ticks rather than the ~15 seconds a cold start took.
 *
 * What makes this affordable is the renderer, not the physics. The old simulation ran at ~8 fps,
 * and almost all of that was the 449 ms canvas2d repaint on every tick — not d3. With painting
 * down to about a millisecond, the tick itself is what is left.
 */

/** Warm-start alpha. Low on purpose: this is a polish pass, and a high alpha would throw away
 *  the structure the server spent 750 ms computing. */
const ALPHA = 0.35
const ALPHA_DECAY = 0.045
const ALPHA_MIN = 0.006
/** Fraction of the server's edge length the client settles toward. Below 1 the graph contracts,
 *  which — together with `NODE_RADIUS_FRACTION_OF_EDGE` — is what stops it reading as dust. */
/**
 * Layout tuning, per mode. 2D is left exactly as it was — its layout was already well judged, and
 * the containment 3D needs makes 2D balloon. 3D alone gets the weaker charge, the shorter
 * repulsion range and the centring pull, because a volume layout otherwise spreads until its
 * nodes are specks against the span the camera has to fit.
 */
const TUNING = {
  2: { chargeDivisor: 30, distanceMaxFactor: 12, linkDistance: 1, linkStrength: 0.35, gravity: 0 },
  3: { chargeDivisor: 55, distanceMaxFactor: 8, linkDistance: 0.85, linkStrength: 0.45, gravity: 0.02 },
} as const

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
  ) => void
  stop: () => void
  readonly running: boolean
}

export function createSimulation(cb: SimulationCallbacks): SimulationHandle {
  let sim: Simulation<GraphNode> | null = null
  let running = false

  function stop() {
    sim?.stop()
    sim = null
    running = false
  }

  function start(
    nodes: GraphNode[],
    links: GraphLink[],
    dimensions: 2 | 3 = 2,
    dist = 30,
  ) {
    stop()
    if (nodes.length < 2) return

    // d3's own defaults sit at distance 30 / strength −30, so preserve that ratio at our scale;
    // charge goes as the square of length for the equilibrium spacing to hold.
    const tuning = TUNING[dimensions]
    // Snapshot the collision radii before the first tick; see the note on the collide force.
    const collideRadii = nodes.map((node) => node.rw * 1.1)
    const charge = -(dist * dist) / tuning.chargeDivisor

    // Entering 3D from a flat layout gives the z-axis nothing to work with — every node sits on
    // the same plane, so the forces have no asymmetry to amplify and the graph stays a pancake.
    // A small seed of depth is what lets it actually inflate.
    if (dimensions === 3) {
      for (const n of nodes) {
        if (!n.z) n.z = (Math.random() - 0.5) * dist
      }
    } else {
      for (const n of nodes) n.z = 0
    }

    // Dimensions must be passed at construction, NOT via `.numDimensions()` afterwards:
    // `forceSimulation` initialises each node's velocity components immediately, so a
    // 2D-initialised node has no `vz` and its `z` becomes NaN on the first tick — which
    // silently poisons the bounds and blanks the canvas.
    sim = forceSimulation<GraphNode>(nodes, dimensions)
      .force(
        'charge',
        forceManyBody<GraphNode>()
          .strength(charge)
          .theta(0.9)
          // Long-range repulsion barely changes a warm-started layout but dominates the cost;
          // capping it is most of what makes a full simulation affordable at this size.
          .distanceMax(dist * tuning.distanceMaxFactor),
      )
      .force(
        'link',
        forceLink<GraphNode, GraphLink>(links)
          .id((d) => d.id)
          .distance(dist * tuning.linkDistance)
          .strength(tuning.linkStrength),
      )
      // The collision pass is what actually separates overlapping nodes — the thing the
      // server's approximation leaves behind.
      //
      // Radii are **frozen at start**, not read live from `rw`. Visual size is derived from the
      // layout's edge length, so a live read would close a feedback loop: bigger radius pushes
      // nodes apart, which lengthens edges, which enlarges the radius again. With the 3D radius
      // exceeding the link distance that loop has gain above 1, and the layout inflates without
      // bound — which is exactly what it was doing.
      .force(
        'collide',
        forceCollide<GraphNode>()
          .radius((d) => collideRadii[d.index ?? 0] ?? 1)
          .iterations(1),
      )
      // Mild pull toward the origin, 3D only (2D's gravity is 0, leaving it untouched). Without
      // it nothing bounds a volume layout's extent — components with no edges between them
      // simply drift apart forever.
      .force('x', tuning.gravity ? forceX<GraphNode>(0).strength(tuning.gravity) : null)
      .force('y', tuning.gravity ? forceY<GraphNode>(0).strength(tuning.gravity) : null)
      .force('z', dimensions === 3 ? forceZ<GraphNode>(0).strength(tuning.gravity) : null)
      .alpha(ALPHA)
      .alphaDecay(ALPHA_DECAY)
      .alphaMin(ALPHA_MIN)

    running = true

    // Reduced motion: converge without animating it. The ticks still run — all at once — so the
    // user is handed a settled graph instead of watching it move.
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      sim.stop()
      sim.tick(Math.ceil(Math.log(ALPHA_MIN / ALPHA) / Math.log(1 - ALPHA_DECAY)))
      cb.onTick()
      stop()
      cb.onEnd()
      return
    }

    sim.on('tick', cb.onTick).on('end', () => {
      running = false
      cb.onEnd()
    })
  }

  return {
    start,
    stop,
    get running() {
      return running
    },
  }
}
