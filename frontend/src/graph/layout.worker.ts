/// <reference lib="webworker" />
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
import { GpuLayout } from './gpu/gpuLayout'
import type { LayoutSettings } from './settings'

/**
 * The force settle, off the main thread.
 *
 * At 60 000 nodes a single tick measured **423 ms** — and there are about a hundred of them. On
 * the main thread that is 30 seconds during which the page renders at 2.4 fps and no input is
 * answered. Nothing about the tick is cheap enough to fix in place: `forceManyBody` and
 * `forceCollide` are both O(n log n) and they are doing real work. The fix is to stop doing it
 * where the frames are.
 *
 * The contract is deliberately narrow — positions in, positions out:
 *
 * - The worker owns plain `{x, y, z, vx, vy, vz}` objects built from the arrays it is sent. It
 *   never sees a `GraphNode`, an id string, or anything from the store.
 * - Links arrive as **index pairs**, not ids, so no map lookup and no string ever crosses the
 *   boundary.
 * - Each tick posts a fresh `Float32Array` of positions, **transferred** rather than copied, so
 *   the 720 KB at 60k costs a pointer hand-off instead of a serialization pass.
 *
 * The main thread's only job per frame becomes: take the newest array, mirror it into the GPU
 * buffers, draw.
 */

interface StartMessage {
  type: 'start'
  /** Packed xyz, three floats per node. */
  positions: Float32Array
  /** Collision radius per node, snapshotted by the caller. */
  radii: Float32Array
  /** Two indices per link, into the node arrays. */
  links: Int32Array
  dimensions: 2 | 3
  dist: number
  settings: LayoutSettings
  /** Run every tick at once and post a single result — the reduced-motion path. */
  immediate: boolean
}

type InMessage = StartMessage | { type: 'stop' }

interface WorkerNode {
  /** d3 stamps this itself, but the collide and link forces read it before the first tick, so it
   *  is written up front and typed as always-present. */
  index: number
  x: number
  y: number
  z: number
}

const ALPHA = 0.35
const ALPHA_DECAY = 0.045
const ALPHA_MIN = 0.006

/** Fixed tuning that is not user-facing; mirrors `simulation.ts`. */
const TUNING = {
  2: { chargeDivisor: 30, distanceMaxFactor: 12 },
  3: { chargeDivisor: 55, distanceMaxFactor: 8 },
} as const

const ctx = self as unknown as DedicatedWorkerGlobalScope

let sim: Simulation<WorkerNode> | null = null
let nodes: WorkerNode[] = []
let dims: 2 | 3 = 2

function stop() {
  sim?.stop()
  sim = null
  nodes = []
}

/** Snapshot the live positions into a fresh array and hand ownership to the main thread. */
function post(type: 'tick' | 'end') {
  const out = new Float32Array(nodes.length * 3)
  for (let i = 0; i < nodes.length; i++) {
    out[i * 3] = nodes[i].x
    out[i * 3 + 1] = nodes[i].y
    out[i * 3 + 2] = dims === 3 ? nodes[i].z : 0
  }
  ctx.postMessage({ type, positions: out }, [out.buffer])
}

/**
 * The alpha schedule, precomputed.
 *
 * Both paths follow it: d3 decays alpha itself, and the GPU is handed the same values so the two
 * converge identically rather than merely similarly.
 */
function alphaSchedule(): number[] {
  const out: number[] = []
  let a = ALPHA
  while (a > ALPHA_MIN) {
    a += (0 - a) * ALPHA_DECAY
    out.push(a)
  }
  return out
}

/**
 * The GPU path.
 *
 * Runs the same schedule in batches, posting positions after each batch so the layout animates
 * rather than appearing at the end. A batch is several ticks because the readback — not the
 * compute — is the synchronization point.
 *
 * Returns `false` when WebGPU is unavailable or the device fails mid-run, and the caller falls
 * through to d3. A device can be lost at any time (driver reset, tab backgrounded on some
 * platforms), so this is a runtime condition, not just a capability check.
 */
const GPU_BATCH = 4
let gpu: GpuLayout | null = null
let gpuChecked = false
let generation = 0

async function startGpu(msg: StartMessage, n: number): Promise<boolean> {
  if (!gpuChecked) {
    gpuChecked = true
    gpu = await GpuLayout.create()
  }
  if (!gpu || !gpu.available) return false

  const tuning = TUNING[msg.dimensions]
  const { dist, settings } = msg
  const mine = ++generation

  try {
    gpu.load({
      positions: msg.positions,
      radii: msg.radii,
      links: msg.links,
      dimensions: msg.dimensions,
      dist,
      charge: (-(dist * dist) / tuning.chargeDivisor) * settings.repel,
      linkDist: dist * settings.linkDistance,
      linkStrength: settings.linkForce,
      centre: settings.centreForce,
      distanceMax: dist * settings.linkDistance * tuning.distanceMaxFactor,
    })

    const schedule = alphaSchedule()
    const batch = msg.immediate ? schedule.length : GPU_BATCH
    const t0 = performance.now()
    for (let i = 0; i < schedule.length; i += batch) {
      // A newer start (or a stop) supersedes this run; abandon it rather than posting positions
      // for a graph that is no longer on screen.
      if (mine !== generation) return true
      const positions = await gpu.run(schedule.slice(i, i + batch))
      const last = i + batch >= schedule.length
      if (mine !== generation) return true
      ctx.postMessage({ type: last ? 'end' : 'tick', positions }, [positions.buffer])
      if (last) {
        // One line, on purpose: which path ran and what it cost is the first thing anyone asks
        // when a layout looks wrong, and guessing from a screenshot is how the last two hours
        // went.
        console.info(
          `[layout] gpu · ${n} nodes · ${schedule.length} ticks · ${(performance.now() - t0).toFixed(0)} ms`,
        )
      }
    }
    return true
  } catch (err) {
    // Mid-run failure with positions already posted is still better than no layout: fall back and
    // let d3 finish from wherever the GPU got to.
    console.warn('[gpu] layout failed, falling back to CPU', err)
    return false
  }
}

/** The d3 path — the fallback, and still the only path without WebGPU. */
function startCpu(msg: StartMessage) {
  const n = msg.positions.length / 3
  dims = msg.dimensions
  const { dist, settings } = msg
  const tuning = TUNING[dims]

  nodes = new Array(n)
  for (let i = 0; i < n; i++) {
    const z = msg.positions[i * 3 + 2]
    nodes[i] = {
      index: i,
      x: msg.positions[i * 3],
      y: msg.positions[i * 3 + 1],
      // Entering 3D from a flat layout gives the z-axis nothing to work with — every node sits
      // on the same plane, so the forces have no asymmetry to amplify and the graph stays a
      // pancake. A small seed of depth is what lets it inflate.
      z: dims === 3 ? (z || (Math.random() - 0.5) * dist) : 0,
    }
  }

  const links = new Array(msg.links.length / 2)
  for (let j = 0; j < links.length; j++) {
    links[j] = { source: msg.links[j * 2], target: msg.links[j * 2 + 1] }
  }

  const charge = (-(dist * dist) / tuning.chargeDivisor) * settings.repel
  // Radii are snapshotted by the caller and never re-read: a live read would close a
  // radius → spacing → radius feedback loop that inflates the layout without bound.
  const radii = msg.radii

  // Dimensions must be passed at construction, NOT via `.numDimensions()` afterwards:
  // `forceSimulation` initialises each node's velocity components immediately, so a
  // 2D-initialised node has no `vz` and its `z` becomes NaN on the first tick.
  sim = forceSimulation<WorkerNode>(nodes, dims)
    .force(
      'charge',
      forceManyBody<WorkerNode>()
        .strength(charge)
        .theta(0.9)
        .distanceMax(dist * settings.linkDistance * tuning.distanceMaxFactor),
    )
    .force(
      'link',
      forceLink<WorkerNode, { source: number; target: number }>(links)
        .id((d) => d.index)
        .distance(dist * settings.linkDistance)
        .strength(settings.linkForce),
    )
    .force(
      'collide',
      forceCollide<WorkerNode>()
        .radius((d) => radii[d.index] ?? 1)
        .iterations(1),
    )
    .force('x', settings.centreForce ? forceX<WorkerNode>(0).strength(settings.centreForce) : null)
    .force('y', settings.centreForce ? forceY<WorkerNode>(0).strength(settings.centreForce) : null)
    .force(
      'z',
      dims === 3 && settings.centreForce
        ? forceZ<WorkerNode>(0).strength(settings.centreForce)
        : null,
    )
    .alpha(ALPHA)
    .alphaDecay(ALPHA_DECAY)
    .alphaMin(ALPHA_MIN)

  if (msg.immediate) {
    sim.stop()
    sim.tick(Math.ceil(Math.log(ALPHA_MIN / ALPHA) / Math.log(1 - ALPHA_DECAY)))
    post('end')
    stop()
    return
  }

  const t0 = performance.now()
  sim.on('tick', () => post('tick')).on('end', () => {
    post('end')
    console.info(
      `[layout] cpu · ${n} nodes · ${(performance.now() - t0).toFixed(0)} ms`,
    )
    stop()
  })
}

function start(msg: StartMessage) {
  stop()
  const n = msg.positions.length / 3
  if (n < 2) {
    ctx.postMessage({ type: 'end', positions: msg.positions }, [msg.positions.buffer])
    return
  }
  // The GPU attempt owns the buffers it is given, so the CPU fallback re-reads them from the
  // message — which is why `startGpu` must not transfer anything it might hand back.
  void startGpu(msg, n).then((handled) => {
    if (!handled) startCpu(msg)
  })
}

ctx.onmessage = (e: MessageEvent<InMessage>) => {
  if (e.data.type === 'start') start(e.data)
  else {
    generation++
    stop()
  }
}
