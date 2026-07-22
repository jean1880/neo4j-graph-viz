// Mirrors the Rust `GraphData` served by GET /api/graph. force-graph mutates x/y/vx/vy at
// runtime, and replaces link source/target with node object references once the sim starts.
export interface GraphNode {
  id: string
  name: string
  label: string
  group: string
  deg: number
  props: Record<string, string>
  x?: number
  y?: number
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

/** A neighbour of a node, with the relationship type and direction (→ outgoing, ← incoming). */
export interface Neighbour {
  node: GraphNode | undefined
  type: string
  dir: '→' | '←'
}
