import shader from './forces.wgsl?raw'

/**
 * The force settle on the GPU.
 *
 * The CPU worker took the settle off the main thread but not out of the wall clock — 423 ms per
 * tick at 60k, ~100 ticks. This runs the same forces as a compute kernel, where the O(n²)
 * repulsion is embarrassingly parallel and the tree that made it tractable on a CPU is simply not
 * needed (see the note in `forces.wgsl` on why brute force is *exact* here, not an approximation).
 *
 * Availability is not assumed anywhere. `create()` returns `null` on any browser without WebGPU,
 * on an adapter request that fails, and on a device that is lost — and the worker falls straight
 * back to d3. Nothing above this layer knows which one ran.
 */

/** Layout inputs, in the flat form the kernel wants. */
export interface GpuLayoutInput {
  /** Packed xyz, three floats per node. */
  positions: Float32Array
  radii: Float32Array
  /** Two node indices per link. */
  links: Int32Array
  dimensions: 2 | 3
  dist: number
  charge: number
  linkDist: number
  linkStrength: number
  centre: number
  distanceMax: number
}

const WORKGROUP = 256
const PARAMS_BYTES = 48

/** d3's defaults, mirrored so the two paths converge the same way. */
const VELOCITY_DECAY = 0.6
const COLLIDE_STRENGTH = 0.5

export class GpuLayout {
  private constructor(
    private readonly device: GPUDevice,
    private readonly pipelines: {
      forces: GPUComputePipeline
      integrate: GPUComputePipeline
      bindGroupLayout: GPUBindGroupLayout
    },
  ) {}

  private n = 0
  private groups = 0
  private bindGroup: GPUBindGroup | null = null
  private pos: GPUBuffer | null = null
  private vel: GPUBuffer | null = null
  private params: GPUBuffer | null = null
  private staging: GPUBuffer | null = null
  private owned: GPUBuffer[] = []
  private lost = false

  /**
   * Acquire a device and compile the kernel, or return `null`.
   *
   * Every failure mode here is a fallback, never an error: no `navigator.gpu`, no adapter, a
   * device that refuses to come up, a shader that will not compile on this driver. A graph viewer
   * that shows nothing because a compute shader failed is worse than one that lays out on a CPU.
   */
  static async create(): Promise<GpuLayout | null> {
    try {
      const gpu = (globalThis.navigator as Navigator | undefined)?.gpu
      if (!gpu) return null
      const adapter = await gpu.requestAdapter({ powerPreference: 'high-performance' })
      if (!adapter) return null
      const device = await adapter.requestDevice()

      const module = device.createShaderModule({ code: shader })
      // Surfaced rather than swallowed: a shader that compiles with errors produces a silently
      // motionless layout, which is indistinguishable from a settle that simply finished.
      const info = await module.getCompilationInfo()
      const errors = info.messages.filter((m) => m.type === 'error')
      if (errors.length) {
        console.warn('[gpu] shader failed to compile, falling back to CPU', errors)
        device.destroy()
        return null
      }

      // **An explicit bind group layout, not `layout: 'auto'`.**
      //
      // With `auto`, every pipeline derives its own layout from the bindings its entry point
      // actually uses — and `integrate` touches only pos/vel/params. The two layouts are then
      // incompatible, so binding one group and switching pipelines mid-pass is a validation
      // error, and the offending dispatch is silently dropped. Declaring the layout once makes
      // both pipelines share it, which is also what lets a single `setBindGroup` serve both.
      const visibility = GPUShaderStage.COMPUTE
      const bindGroupLayout = device.createBindGroupLayout({
        entries: [
          { binding: 0, visibility, buffer: { type: 'storage' } },
          { binding: 1, visibility, buffer: { type: 'storage' } },
          { binding: 2, visibility, buffer: { type: 'read-only-storage' } },
          { binding: 3, visibility, buffer: { type: 'read-only-storage' } },
          { binding: 4, visibility, buffer: { type: 'read-only-storage' } },
          { binding: 5, visibility, buffer: { type: 'read-only-storage' } },
          { binding: 6, visibility, buffer: { type: 'uniform' } },
        ],
      })
      const layout = device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] })
      const pipelines = {
        forces: device.createComputePipeline({ layout, compute: { module, entryPoint: 'forces' } }),
        integrate: device.createComputePipeline({
          layout,
          compute: { module, entryPoint: 'integrate' },
        }),
        bindGroupLayout,
      }

      // Validation errors are asynchronous and, by default, land nowhere the app can see. A
      // dropped dispatch looks exactly like a settle that converged instantly.
      device.onuncapturederror = (e) => {
        console.warn('[gpu] device error', (e as GPUUncapturedErrorEvent).error.message)
      }
      const self = new GpuLayout(device, pipelines)
      device.lost.then(() => {
        self.lost = true
      })
      return self
    } catch {
      return null
    }
  }

  get available(): boolean {
    return !this.lost
  }

  /** Upload a graph and reset velocities. Replaces whatever was loaded before. */
  load(input: GpuLayoutInput): void {
    this.release()
    const { device } = this
    const n = input.positions.length / 3
    this.n = n
    this.groups = Math.ceil(n / WORKGROUP)

    // vec4 for alignment; w is unused.
    const packed = new Float32Array(n * 4)
    for (let i = 0; i < n; i++) {
      packed[i * 4] = input.positions[i * 3]
      packed[i * 4 + 1] = input.positions[i * 3 + 1]
      packed[i * 4 + 2] = input.dimensions === 3 ? input.positions[i * 3 + 2] : 0
    }

    const { offsets, targets, bias } = buildCsr(input.links, n)

    const storage = GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST
    this.pos = this.make(packed, storage | GPUBufferUsage.COPY_SRC)
    this.vel = this.make(new Float32Array(n * 4), storage)
    const radii = this.make(input.radii, storage)
    const off = this.make(offsets, storage)
    const tgt = this.make(targets, storage)
    const bs = this.make(bias, storage)

    this.params = device.createBuffer({
      size: PARAMS_BYTES,
      usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST,
    })
    this.owned.push(this.params)

    this.staging = device.createBuffer({
      size: n * 16,
      usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
    })
    this.owned.push(this.staging)

    this.bindGroup = device.createBindGroup({
      layout: this.pipelines.bindGroupLayout,
      entries: [
        { binding: 0, resource: { buffer: this.pos } },
        { binding: 1, resource: { buffer: this.vel } },
        { binding: 2, resource: { buffer: radii } },
        { binding: 3, resource: { buffer: off } },
        { binding: 4, resource: { buffer: tgt } },
        { binding: 5, resource: { buffer: bs } },
        { binding: 6, resource: { buffer: this.params } },
      ],
    })

    this.config = input
  }

  private config: GpuLayoutInput | null = null

  /**
   * Run `ticks` steps at the given alpha schedule and return the resulting positions.
   *
   * Batching matters: the readback is the only synchronization point, so running several ticks
   * per readback converges in a fraction of the wall time while still handing the caller
   * something to draw every frame.
   */
  async run(alphas: number[]): Promise<Float32Array> {
    const { device, config } = this
    if (!config || !this.bindGroup || !this.pos || !this.params || !this.staging) {
      throw new Error('gpu layout: run before load')
    }

    const params = new ArrayBuffer(PARAMS_BYTES)
    const u32 = new Uint32Array(params)
    const f32 = new Float32Array(params)

    // **One submit per tick, deliberately.** `queue.writeBuffer` and `queue.submit` are ordered
    // against each other, but writes are not interleaved with passes *inside* a single command
    // buffer: encoding every tick into one encoder and writing the uniform between them would
    // leave every tick reading whichever alpha was written last. The schedule would collapse to
    // its final value and the layout would barely move. Per-tick submits cost a handful of
    // microseconds each and are still fully asynchronous — the only stall is the readback below.
    for (const alpha of alphas) {
      u32[0] = this.n
      u32[1] = config.dimensions
      f32[4] = alpha
      f32[5] = config.charge
      f32[6] = config.linkDist
      f32[7] = config.linkStrength
      f32[8] = config.centre
      f32[9] = config.distanceMax
      f32[10] = VELOCITY_DECAY
      f32[11] = COLLIDE_STRENGTH
      device.queue.writeBuffer(this.params, 0, params.slice(0))

      const encoder = device.createCommandEncoder()
      const pass = encoder.beginComputePass()
      pass.setBindGroup(0, this.bindGroup)
      pass.setPipeline(this.pipelines.forces)
      pass.dispatchWorkgroups(this.groups)
      pass.setPipeline(this.pipelines.integrate)
      pass.dispatchWorkgroups(this.groups)
      pass.end()
      device.queue.submit([encoder.finish()])
    }

    const copy = device.createCommandEncoder()
    copy.copyBufferToBuffer(this.pos, 0, this.staging, 0, this.n * 16)
    device.queue.submit([copy.finish()])

    await this.staging.mapAsync(GPUMapMode.READ)
    const packed = new Float32Array(this.staging.getMappedRange().slice(0))
    this.staging.unmap()

    const out = new Float32Array(this.n * 3)
    for (let i = 0; i < this.n; i++) {
      out[i * 3] = packed[i * 4]
      out[i * 3 + 1] = packed[i * 4 + 1]
      out[i * 3 + 2] = packed[i * 4 + 2]
    }
    return out
  }

  /** Free every buffer for the loaded graph. The device and pipelines survive. */
  release(): void {
    for (const b of this.owned) b.destroy()
    this.owned = []
    this.bindGroup = null
    this.pos = null
    this.vel = null
    this.params = null
    this.staging = null
    this.config = null
  }

  destroy(): void {
    this.release()
    this.device.destroy()
  }

  private make(data: Float32Array | Uint32Array, usage: GPUBufferUsageFlags): GPUBuffer {
    const buffer = this.device.createBuffer({
      // WebGPU requires a multiple of 4; every array here is already 4-byte typed, but an empty
      // one would be size 0, which is rejected.
      size: Math.max(4, data.byteLength),
      usage,
    })
    this.device.queue.writeBuffer(buffer, 0, data as unknown as ArrayBufferView<ArrayBuffer>)
    this.owned.push(buffer)
    return buffer
  }
}

/**
 * Undirected edge list → CSR adjacency, with d3's degree bias baked in per entry.
 *
 * d3 splits a link's correction between its endpoints by degree — the busier end moves less —
 * via `bias = deg(source) / (deg(source) + deg(target))`. Precomputing it here keeps the shader
 * free of any degree lookup.
 */
function buildCsr(links: Int32Array, n: number) {
  const m = links.length / 2
  const degree = new Uint32Array(n)
  for (let e = 0; e < m; e++) {
    degree[links[e * 2]]++
    degree[links[e * 2 + 1]]++
  }

  const offsets = new Uint32Array(n + 1)
  for (let i = 0; i < n; i++) offsets[i + 1] = offsets[i] + degree[i]

  const total = offsets[n]
  const targets = new Uint32Array(Math.max(1, total))
  const bias = new Float32Array(Math.max(1, total))
  const cursor = offsets.slice(0, n)

  for (let e = 0; e < m; e++) {
    const s = links[e * 2]
    const t = links[e * 2 + 1]
    const ds = degree[s]
    const dt = degree[t]
    const sum = ds + dt || 1
    // Each direction stores the bias to apply to *its own* endpoint.
    targets[cursor[s]] = t
    bias[cursor[s]] = dt / sum
    cursor[s]++
    targets[cursor[t]] = s
    bias[cursor[t]] = ds / sum
    cursor[t]++
  }

  return { offsets, targets, bias }
}
