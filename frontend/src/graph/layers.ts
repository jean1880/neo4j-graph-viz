import type { Layer, PickingInfo } from '@deck.gl/core'
import { LineLayer, ScatterplotLayer } from '@deck.gl/layers'
import { SimpleMeshLayer } from '@deck.gl/mesh-layers'
import { SphereGeometry } from '@luma.gl/engine'
import type { Buffers } from './buffers'

/**
 * Sphere mesh for 3D nodes.
 *
 * Built once and instanced across every node, so the segment count is paid once in memory and
 * 25 000 times in triangles — kept deliberately low, because at this instance count geometry
 * detail costs far more than it shows. A disc would be cheaper still, but a billboard stays
 * flat however you orbit it, which defeats the point of the mode.
 */
const SPHERE = new SphereGeometry({ radius: 1, nlat: 8, nlong: 12 })

/** A single point carrying the selection ring. */
export interface Ring {
  pos: [number, number, number]
  r: number
}

/**
 * Stable layer ids.
 *
 * These must NOT vary with state. deck.gl tracks "what is currently hovered" *per layer id*, so
 * an id that changes when the highlight changes destroys that tracking: the old layer is gone
 * before it can report the pointer leaving, and the hover sticks forever. Re-uploading is
 * signalled by `data` object identity instead, which changes on every build below.
 */
/**
 * The two node layers are different deck.gl layer *types*, so they must not share an id: given
 * one id, deck.gl tries to update the scatterplot into a mesh and carries over accessors the
 * mesh has never heard of (`getRadius`). Distinct ids, each stable across state changes.
 */
const NODES_LAYER_ID_2D = 'nodes-2d'
const NODES_LAYER_ID_3D = 'nodes-3d'

/** Whether a picked layer id belongs to the nodes — the only pickable thing on the canvas. */
export function isNodesLayer(id: string | undefined): boolean {
  return id === NODES_LAYER_ID_2D || id === NODES_LAYER_ID_3D
}

export interface LayerParams {
  buf: Buffers
  /** Bumped whenever a buffer is rewritten. Only used to keep the `data` objects below fresh,
   *  which is what tells deck.gl to re-read the (mutated) typed arrays. */
  revision: number
  ring: Ring[]
  /** 3 renders spheres through a mesh layer; 2 renders flat discs. */
  dimensions: 2 | 3
  /** Edge stroke width in pixels, from the display sliders. */
  linkThickness: number
  /** The node-size slider. Scales the 2D pixel floor as well as the world radius — see below. */
  nodeSize: number
  onHover: (info: PickingInfo) => void
  onClick: (info: PickingInfo) => void
}

/**
 * Build the deck.gl layer stack.
 *
 * Nodes and links are fed as **binary attributes** — no per-node accessor, so deck.gl never
 * calls back into JavaScript 25 000 times to find out what to draw. The two small layers
 * (selection ring, labels) use ordinary accessors because their data is a handful of items.
 */
export function buildLayers(p: LayerParams): Layer[] {
  const { buf } = p
  // Referenced so each build produces distinct `data` wrappers; see the note on ids above.
  void p.revision
  return [
    new LineLayer({
      id: 'links',
      data: {
        length: buf.m,
        attributes: {
          getSourcePosition: { value: buf.linkSource, size: 3 },
          getTargetPosition: { value: buf.linkTarget, size: 3 },
          getColor: { value: buf.linkColour, size: 4 },
        },
      },
      widthUnits: 'pixels',
      // Width is the other half of legibility: a thicker stroke reads at a lower opacity than a
      // hairline does, which is what lets the density scaling stay conservative. The 3D default
      // is higher, because an angled edge covers fewer pixels than the same edge face-on — see
      // `DEFAULT_SETTINGS`, which is where both defaults now live.
      getWidth: p.linkThickness,
      pickable: false,
    }),
    p.dimensions === 3
      ? new SimpleMeshLayer({
          id: NODES_LAYER_ID_3D,
          data: {
            length: buf.n,
            attributes: {
              getPosition: { value: buf.positions, size: 3 },
              getScale: { value: buf.scale, size: 3 },
              getColor: { value: buf.colour, size: 4 },
            },
          },
          mesh: SPHERE,
          // The magnitude lives here, in a plain prop, precisely because a binary `getScale`
          // deck.gl declines to consume fails silently — see the note on `Buffers.scale`.
          sizeScale: buf.meshSizeScale,
          // Lit so depth actually reads: an unlit sphere is a flat circle again.
          material: { ambient: 0.45, diffuse: 0.7, shininess: 24, specularColor: [50, 50, 55] },
          pickable: true,
          onHover: p.onHover,
          onClick: p.onClick,
        })
      : new ScatterplotLayer({
          id: NODES_LAYER_ID_2D,
          data: {
            length: buf.n,
            attributes: {
              getPosition: { value: buf.positions, size: 3 },
              getRadius: { value: buf.radius, size: 1 },
              getFillColor: { value: buf.colour, size: 4 },
            },
          },
          radiusUnits: 'common',
          // A big layout viewed whole makes world-unit radii sub-pixel; without a floor the
          // nodes vanish and only the edges render.
          //
          // The floor **scales with the size slider**, and has to. Zoomed out on a large graph
          // every node is already clamped here, so a slider that only changed the world radius
          // moved a number the floor then overrode — the control looked broken precisely when
          // the whole graph was on screen, which is when you reach for it.
          radiusMinPixels: 2 * p.nodeSize,
          pickable: true,
          onHover: p.onHover,
          onClick: p.onClick,
        }),
    // The selection ring is its own one-point layer — far cheaper than giving every node a
    // stroke it almost never uses.
    new ScatterplotLayer({
      id: 'selection',
      data: p.ring,
      getPosition: (d: Ring) => d.pos,
      getRadius: (d: Ring) => d.r * 1.35,
      radiusUnits: 'common',
      radiusMinPixels: 3,
      stroked: true,
      filled: false,
      getLineColor: [255, 255, 255, 235],
      lineWidthUnits: 'pixels',
      getLineWidth: 2,
      pickable: false,
    }),
  ] as Layer[]
}
