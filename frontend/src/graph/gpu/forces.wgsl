// The force settle as a compute kernel.
//
// Mirrors d3-force's semantics deliberately, force by force, so a graph laid out here and a graph
// laid out on the CPU fallback are the same graph. Where it differs, it differs knowingly:
//
//  - **Repulsion is brute force, not Barnes–Hut.** d3 caps repulsion at `distanceMax`, so every
//    pair beyond that contributes exactly nothing — which makes the O(n²) loop *exact* rather than
//    an approximation, and removes the tree build entirely. The GPU would rather do 3.6 billion
//    trivially parallel rejections than build a pointer structure.
//  - **Collision is a velocity impulse**, where d3 resolves it positionally over iterations. The
//    symmetric impulse converges to the same separation without the order dependence that makes
//    d3's version sensitive to node ordering.
//
// Positions are `vec4` purely for alignment; `w` is unused. In 2D every `z` is forced to zero at
// every step rather than trusted to stay there — one NaN in `z` propagates into the bounds and
// blanks the canvas.

struct Params {
  n: u32,
  dims: u32,
  _pad0: u32,
  _pad1: u32,
  alpha: f32,
  charge: f32,
  linkDist: f32,
  linkStrength: f32,
  centre: f32,
  distanceMax: f32,
  velocityDecay: f32,
  collideStrength: f32,
};

@group(0) @binding(0) var<storage, read_write> pos: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> vel: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> radius: array<f32>;
// CSR adjacency: every node's edges are contiguous, so the link force needs no atomics and no
// scatter — each thread accumulates only into its own node.
@group(0) @binding(3) var<storage, read> adjOffset: array<u32>;
@group(0) @binding(4) var<storage, read> adjTarget: array<u32>;
@group(0) @binding(5) var<storage, read> adjBias: array<f32>;
@group(0) @binding(6) var<uniform> P: Params;

const TILE: u32 = 256u;

// One tile of node positions and radii, staged in workgroup memory so the O(n²) loop reads global
// memory n/TILE times instead of n times. This is the whole reason brute force is affordable.
var<workgroup> tilePos: array<vec4<f32>, 256>;
var<workgroup> tileRad: array<f32, 256>;

@compute @workgroup_size(256)
fn forces(
  @builtin(global_invocation_id) gid: vec3<u32>,
  @builtin(local_invocation_id) lid: vec3<u32>,
) {
  let i = gid.x;
  let n = P.n;
  let flat = P.dims == 2u;

  var p = vec3<f32>(0.0, 0.0, 0.0);
  var ri = 0.0;
  if (i < n) {
    p = pos[i].xyz;
    ri = radius[i];
  }
  var acc = vec3<f32>(0.0, 0.0, 0.0);

  let maxSq = P.distanceMax * P.distanceMax;
  let tiles = (n + TILE - 1u) / TILE;

  for (var t: u32 = 0u; t < tiles; t = t + 1u) {
    let j = t * TILE + lid.x;
    if (j < n) {
      tilePos[lid.x] = pos[j];
      tileRad[lid.x] = radius[j];
    } else {
      tilePos[lid.x] = vec4<f32>(0.0, 0.0, 0.0, 0.0);
      tileRad[lid.x] = 0.0;
    }
    // Every invocation must reach both barriers, so they sit outside the `i < n` guard.
    workgroupBarrier();

    if (i < n) {
      let count = min(TILE, n - t * TILE);
      for (var k: u32 = 0u; k < count; k = k + 1u) {
        let jj = t * TILE + k;
        if (jj == i) {
          continue;
        }
        // From j toward i: the direction i is pushed when the two repel.
        var d = p - tilePos[k].xyz;
        if (flat) {
          d.z = 0.0;
        }
        var l2 = dot(d, d);
        if (l2 < 1e-6) {
          // Exactly coincident nodes have no direction to separate along. d3 jiggles randomly;
          // a shader has no RNG, so the index supplies a deterministic offset — which has the
          // side benefit of making a layout reproducible run to run.
          d = vec3<f32>(f32(i % 13u) - 6.0, f32(jj % 11u) - 5.0, select(0.0, f32(i % 7u) - 3.0, !flat)) * 1e-3;
          l2 = dot(d, d) + 1e-9;
        }

        // Charge. `P.charge` is negative, so subtracting pushes i away from j — the same sign
        // convention as d3's `vx += dx * strength * alpha / distSq`.
        if (l2 < maxSq) {
          acc = acc - d * (P.charge * P.alpha / l2);
        }

        // Collision: separate only where the discs actually overlap.
        let rsum = ri + tileRad[k];
        if (l2 < rsum * rsum) {
          let l = sqrt(l2);
          acc = acc + d * ((rsum - l) / l * P.collideStrength);
        }
      }
    }
    workgroupBarrier();
  }

  if (i >= n) {
    return;
  }

  // Links, straight off the CSR row for this node.
  let start = adjOffset[i];
  let end = adjOffset[i + 1u];
  for (var k: u32 = start; k < end; k = k + 1u) {
    var d = pos[adjTarget[k]].xyz - p;
    if (flat) {
      d.z = 0.0;
    }
    let l = max(length(d), 1e-6);
    acc = acc + d * ((l - P.linkDist) / l * P.alpha * P.linkStrength * adjBias[k]);
  }

  // Pull toward the origin. Zero in 2D by default, which makes this a no-op there.
  if (P.centre > 0.0) {
    acc = acc - p * (P.centre * P.alpha);
  }

  var v = (vel[i].xyz + acc) * P.velocityDecay;
  if (flat) {
    v.z = 0.0;
  }
  vel[i] = vec4<f32>(v, 0.0);
}

// Integration is its own dispatch, not the tail of the force pass: every thread must read the
// *same* position snapshot, and a thread that had already moved would corrupt its neighbours'
// forces for the rest of the tick.
@compute @workgroup_size(256)
fn integrate(@builtin(global_invocation_id) gid: vec3<u32>) {
  let i = gid.x;
  if (i >= P.n) {
    return;
  }
  var p = pos[i].xyz + vel[i].xyz;
  if (P.dims == 2u) {
    p.z = 0.0;
  }
  pos[i] = vec4<f32>(p, 0.0);
}
