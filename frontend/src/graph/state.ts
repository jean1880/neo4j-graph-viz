import type { GraphNode } from '../types'

/**
 * Everything that decides what the canvas shows, in one object.
 *
 * The renderer modules take this rather than reaching into the store themselves: it keeps them
 * pure functions of their inputs (so they can be reasoned about and tested in isolation) and it
 * gives the component a single place to answer "what is on screen right now".
 */
export interface DrawState {
  nodes: GraphNode[]
  /** Labels the legend has switched off. */
  hidden: ReadonlySet<string>
  /** Survivors of the active search, or `null` when no search is running. Anything outside this
   *  set is *removed* from the canvas, not dimmed. */
  surviving: ReadonlySet<string> | null
  focusId: string | null
  focusNodeIds: ReadonlySet<string>
  /** Lowercased search term, for the match highlight. */
  term: string
  /** 2D or 3D. Some styling has to differ: a 3D layout occupies far more space, and its edges
   *  overlap in depth, so the same alpha that reads as a hint in 2D becomes fog. */
  dimensions: 2 | 3
  /** The one label type left visible, when the legend has isolated a single type. Labels are
   *  off by default; deliberately narrowing to one type is one of the few things that turns
   *  them back on. */
  isolatedLabel: string | null
}

/** Whether a node is drawn at all: not hidden by the legend, and not filtered out by a search. */
export function isDrawn(state: DrawState, node: GraphNode): boolean {
  if (state.hidden.has(node.label)) return false
  return state.surviving === null || state.surviving.has(node.id)
}
