// Mirrors the Rust `GraphData` served by GET /api/graph. Nothing here is mutated at runtime:
// coordinates arrive already laid out by the backend, and links keep their string endpoints
// (the old force simulation used to rewrite both in place, ~60x/s — it no longer exists).
//
// Node *properties* are deliberately absent: at 25k nodes the property bags dominate the
// payload and the browser blocks parsing them before it can draw. They are fetched per node
// from `GET /api/node/{id}` when the detail panel opens — see `NodeDetailData`.

/** A node exactly as `GET /api/graph` sends it. */
export interface RawGraphNode {
  id: string
  name: string
  label: string
  group: string
  deg: number
  /** Layout coordinates, computed by the backend. */
  x: number
  y: number
  /** Depth. Absent from the payload — the backend lays out in 2D — and filled in by the client
   *  simulation when the view is in 3D. Zero in 2D. */
  z?: number
}

/** A node after `useGraph` stamps the derived render fields onto it at load. */
export interface GraphNode extends RawGraphNode {
  /** Lowercased `name`. The canvas tests every node against the search term on every frame;
   *  computing this there cost one string allocation per node per frame. */
  nameLower: string
  /** Degree-derived size *weight*, not a length. Relative only — a hub is bigger than a leaf.
   *  `deg` never changes after load, so recomputing `Math.sqrt` every frame was pure waste. */
  r: number
  /** Drawn radius in **world units**, set by `buildBuffers` from `r` and the layout's own edge
   *  length. What actually reads on screen is the ratio of node size to node spacing, so a
   *  radius fixed in absolute units looks right at one layout scale and lost at every other. */
  rw: number
  /** Set by d3-force at simulation start — this node's position in the simulated array. Not part
   *  of the API payload; it exists because the force library puts it there. */
  index?: number
}

/** `GET /api/node/{id}` — one node *with* its properties. */
export interface NodeDetailData extends RawGraphNode {
  props: Record<string, string>
}

export interface GraphLink {
  source: string | GraphNode
  target: string | GraphNode
  type: string
}

export interface GraphData {
  nodes: GraphNode[]
  links: GraphLink[]
}

/** One ranked search hit. */
export interface SearchMatch {
  id: string
  name: string
  label: string
  score: number
}

/** A node that survived a search, with the position it should be drawn at. Coordinates come
 *  back re-laid-out over the surviving subgraph, so results render compactly rather than
 *  scattered across the holes left by everything that was removed. */
export interface VisibleNode {
  id: string
  x: number
  y: number
}

/** `GET /api/search` — the subgraph related to a query. */
export interface SearchResponse {
  query: string
  matches: SearchMatch[]
  visible: VisibleNode[]
  /** The visible set hit the server's ceiling and was cut short. */
  truncated: boolean
  /** Whether semantic (embedding) scoring contributed, or it was purely lexical. */
  semantic: boolean
}

/** Timings for one graph load, in milliseconds (plus the payload size in bytes). */
export interface LoadPerf {
  transferMs: number
  parseMs: number
  stampMs: number
  totalMs: number
  bytes: number
  nodes: number
  links: number
}

/** A neighbour of a node, with the relationship type and direction (→ outgoing, ← incoming). */
export interface Neighbour {
  node: GraphNode | undefined
  type: string
  dir: '→' | '←'
}
