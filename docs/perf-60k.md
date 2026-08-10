# Performance work — 60k nodes

Continues `performance-plan.md`, which took the renderer to 17.4 ms p95 at 25k. This file tracks
the next tier, aimed at ~60 000 nodes / ~180 000 edges.

Everything here is measured against the synthetic fixture (`make bench BENCH_NODES=60000`), so it
needs no Neo4j and no credentials, and the numbers are reproducible against a graph that has not
changed shape between runs.

## The ranked list

| # | Change | Why | Status |
|---|---|---|---|
| 2 | CPU picking (spatial index) instead of deck.gl's picking pass | deck re-renders the whole scene into a picking framebuffer on every pointer move | **dropped — measured free** |
| 1 | Indexed edges — endpoints as indices, positions sampled in the shader | `linkSource`/`linkTarget` duplicate positions and are rewritten every frame (~4.3 MB/frame at 60k) | planned |
| 3 | Split static colour from a 1-byte interaction-state attribute | a hover rewrites ~960 KB of RGBA to change a highlight | planned |
| 4 | Sphere impostors in 3D | 96 tris/node × 60k = 5.8M triangles per frame | planned |
| 5 | Edge LOD / subsampling by zoom | 180k alpha-blended lines is a fill-rate problem | planned |
| 6 | Settle in a Web Worker | O(n log n) × ~100 ticks blocks the main thread for seconds | **done** |
| 2b | WebGPU compute settle | the worker fixed responsiveness, not wall time | **done** |
| 7 | Binary transport | `performance-plan.md` Phase 3, still open | planned |

## Baseline — 60k fixture, and why the ranking above was wrong

Measured on the 60 000-node fixture (`GRAPH_FIXTURE_NODES=60000 GRAPH_FIXTURE_EDGES=3`), Chromium,
RTX 4080, 2560×1265 canvas.

| | |
|---|---|
| `/api/graph` payload | **19.5 MB** JSON |
| First request (incl. server layout) | 2.01 s |
| Cached request, loopback | 39 ms |
| **Frames during the settle** | **median 423 ms, p95 451 ms — 2.4 fps** |
| Long tasks during a 30 s settle | **70 tasks, 29.7 s total** — the main thread is ~100% blocked |
| Frames post-settle, idle | 6.9 ms (display cadence; nothing repaints) |
| Frames post-settle, **hover storm** | **6.9 ms** — unchanged |

**The renderer is not the problem, and neither is picking.** A hover storm (200 synthetic pointer
moves, tooltip confirmed resolving on every one) did not move frame time at all — so deck.gl's
picking pass, the `restyle` buffer rewrite, and the ~6 MB/frame upload together cost **under 7 ms**
at 60k. Items 1, 2 and 3 in the table above were ranked on arithmetic, not measurement, and the
arithmetic was beside the point.

**The client force settle is the entire problem.** ~423 ms per tick × ~100 ticks, all on the main
thread. That is where the 30 s of blocking goes, and it is why the page is unusable while it
happens. Item 6 was ranked last and should have been first.

Revised order: **6 (worker) → re-measure → everything else only if it then shows up.**

## Done — the settle moved to a Web Worker

`src/graph/layout.worker.ts` runs the force pass; `src/graph/simulation.ts` became a thin driver
with the same `SimulationHandle` API, so the canvas barely changed. Transport is positions in,
positions out: `Float32Array` of xyz plus `Int32Array` index pairs, **transferred** rather than
cloned, so a 60k update is a pointer hand-off. No ids, no objects, nothing from the store crosses
the boundary.

| 60k fixture | Before | After |
|---|---|---|
| Frame time during settle | 423 ms median / 451 ms p95 | **16.6 ms median / 16.7 ms p95** |
| Effective frame rate | 2.4 fps | **60 fps, vsync-locked** |
| Long tasks over 30 s | 70, totalling **29.7 s** | **0** |
| Frames rendered in 30 s | 72 | **2 114** |

### Two traps this hit, both worth remembering

**A `postMessage` of a Vue reactive object throws `DataCloneError` — and the failure is silent.**
`settings` is a `reactive()` Proxy; structured clone refuses it, the message is never delivered,
and the settle simply never happens. The page looks *fine* (it shows the server's layout forever)
and every frame-time metric reports a triumph: the first "after" measurement came back at 16.7 ms
with zero long tasks purely because nothing was running. Settings are now copied field by field.

**A synthetic-event probe is not proof.** The follow-up check — dispatching `MouseEvent`s and
counting tooltips — returned 0 hits out of 120 and looked like the worker had blanked the canvas.
It hadn't: a screenshot showed the graph rendering correctly. The probe had stopped reaching
deck.gl's pointer handling after the tab was resized. **Verify with an artefact you can look at**
(a screenshot, a byte hash of two frames) before believing an instrument you wrote five minutes
ago.

## Done — the settle moved to the GPU (WebGPU compute)

`src/graph/gpu/forces.wgsl` + `src/graph/gpu/gpuLayout.ts`. The worker tries WebGPU first and
falls back to d3 on any failure, so nothing above the worker knows which ran. One line of console
output says which did, and what it cost:

```
[layout] gpu · 60000 nodes · 89 ticks · 1218 ms
```

**Repulsion is brute force O(n²), and that is not a compromise.** d3 caps repulsion at
`distanceMax`, so every pair beyond it contributes exactly zero — the full loop computes the same
answer the Barnes–Hut tree approximates, and the GPU would rather do 3.6 billion trivially
parallel rejections than build a pointer structure. Collision becomes a symmetric velocity impulse
instead of d3's iterative positional pass, which also removes its dependence on node ordering.

| Full settle, 60k | Wall time | UI during |
|---|---|---|
| d3, main thread (original) | ~30 s | 2.4 fps, frozen |
| d3, worker | ~30 s | 60 fps |
| **WebGPU compute** | **1.2 s** | **60 fps** |

Scaling, with ~60 ms of fixed readback overhead: 30k → 450 ms, 60k → 1218 ms, i.e. ~3.0× for 4×
the pairs. Quadratic in the compute term, as it should be. 3D verified separately — real
volumetric layout, not a flattened one.

### The bug that made the first GPU number a lie

The first run reported **68 ms** at 60k, and 75 ms at *120k* — four times the pairs for 10% more
time, which is not physically possible. The cause was `layout: 'auto'` on both compute pipelines.
With `auto`, each pipeline derives a bind group layout from the bindings its own entry point
touches, and `integrate` touches only pos/vel/params. The two layouts are then incompatible, so
binding one group and switching pipelines mid-pass is a validation error — and WebGPU validation
errors are asynchronous and go nowhere by default, so the dispatch was **silently dropped**.

Fixed with an explicit shared `GPUBindGroupLayout`, plus a `device.onuncapturederror` handler so
the next one is not silent. 68 ms → 1218 ms, which is what doing the work actually costs.

**Impossible-looking scaling is the tell.** Both times a GPU number looked too good, the work was
not happening. Check that timings scale the way the algorithm says they must before believing
them.

### Framing at this size — resolved by animating it

At 60k the settle expands the layout well past its starting size, so the framing chosen before the
first tick is badly wrong by the end. The post-settle refit had been removed because it *snapped*:
the graph was in one place and, one frame later, somewhere else.

The framing was never the problem — the discontinuity was. The refit is back, travelled over
550 ms with an ease-in-out, and cancelled the moment the user touches the camera. Verified by
hashing the canvas frame by frame during a fit: 9 distinct frames with the last change at exactly
550 ms, where a snap gives 2 distinct frames and stops at ~16 ms.

Driven by our own scheduler rather than deck.gl's `transitionDuration`, because `paint` writes
`viewState` every frame — deck treats each write as a new target and restarts its own transition,
so its interpolators cannot survive this render loop. Zoom is interpolated in log2 units (a lerp
of the linear scale crawls at the wide end and stampedes at the close end) and `rotationOrbit`
takes the short way round.


## Bug — switching to 3D while zoomed in rendered nothing

Symptom: zoom in far in 2D, click 3D, get a blank canvas. It stayed blank indefinitely. From the
default framing (no prior zoom) 3D was fine, which is what made it look intermittent.

Cause, from deck's own log: `Pixel project matrix not invertible`. 2D clamps at `maxZoom: 12`, and
handing a scale of 2^12 to `OrbitView` collapses its near/far planes into a degenerate projection,
so deck draws nothing. **An orthographic `zoom` and an orbit `zoom` are not the same quantity** —
carrying one across the switch was never meaningful, and there is nothing to preserve anyway since
the two modes are different layouts in different coordinates.

Fixed by resetting the camera to `INITIAL_VIEW` in the dimension watcher, letting the post-load fit
frame the new layout from a neutral pose.

Two wrong diagnoses came first, both worth recording because each *looked* confirmed:

1. **"The tween flies through empty space."** A mid-transition screenshot was blank, which fit the
   theory that interpolating across a content replacement is meaningless. That reasoning is
   correct and the reload path does now snap deliberately — but it was not the bug: the canvas was
   still blank with the tween removed.
2. **"`userMovedCamera` is latched by deck's own view-class change."** Also real — deck emits
   `onViewStateChange` for its own bookkeeping, and gating on `interactionState` (isDragging /
   isPanning / isZooming / isRotating) is a genuine correctness fix that was kept. But it did not
   fix the blank canvas either.

The console warning was there the whole time and named the cause exactly. Both wrong turns came
from reasoning about the symptom before reading what the library was already saying.
