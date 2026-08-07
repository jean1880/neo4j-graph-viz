import { positionAt, type Buffers } from './buffers'
import { worldHalfExtent, type ViewState } from './camera'
import { isDrawn, type DrawState } from './state'

/**
 * Label placement — **off by default**.
 *
 * At 25 000 nodes, labelling everything in view produces overlapping mush that is slower to
 * render and harder to read than no labels at all. So text is not a background feature of the
 * map; it appears only when you have asked a question that names something:
 *
 * 1. **hover / selection** — the node under focus and its neighbourhood
 * 2. **search** — whatever matched
 * 3. **an isolated node type** — the legend narrowed to a single type
 *
 * With none of those active the graph renders as pure shape, which is what it is good at.
 */
export interface Label {
  /** The node this label names — so clicking the label can select that node. */
  id: string
  name: string
  deg: number
  x: number
  y: number
  z: number
  r: number
}

/** Ceiling for any single reason to show labels. A hub with 600 neighbours must not carpet the
 *  screen just because the cursor crossed it. */
const LABEL_BUDGET = 160
/** Below this zoom individual names are noise however few of them there are. */
const LABEL_MIN_ZOOM = -3

export function buildLabels(
  buf: Buffers,
  state: DrawState,
  view: ViewState,
  width: number,
  height: number,
): Label[] {
  const { nodes, focusId, focusNodeIds, term, isolatedLabel } = state

  // Nothing named, nothing to label — the common case, and the cheapest possible answer.
  const searching = term !== ''
  if (focusId === null && !searching && isolatedLabel === null) return []
  if (view.zoom < LABEL_MIN_ZOOM) return []

  const label = (i: number): Label => {
    const [x, y, z] = positionAt(buf, i)
    return { id: nodes[i].id, name: nodes[i].name, deg: nodes[i].deg, x, y, z, r: nodes[i].rw }
  }

  const [halfW, halfH] = worldHalfExtent(view, width, height)
  const [cx, cy] = view.target
  const onScreen = (x: number, y: number) =>
    Math.abs(x - cx) <= halfW && Math.abs(y - cy) <= halfH

  // The focused node itself is never dropped, whatever else competes for the budget.
  const out: Label[] = []
  const taken = new Set<string>()
  const push = (i: number) => {
    if (taken.has(nodes[i].id)) return
    taken.add(nodes[i].id)
    out.push(label(i))
  }

  const focusIdx = focusId !== null ? buf.idIndex.get(focusId) : undefined
  if (focusIdx !== undefined) push(focusIdx)

  // Candidates from whichever reasons are active, ranked by degree so the most connected — and
  // therefore most orienting — names win the budget.
  const candidates: number[] = []
  for (let i = 0; i < buf.n; i++) {
    const node = nodes[i]
    if (!isDrawn(state, node) || taken.has(node.id)) continue
    const wanted =
      (searching && node.nameLower.includes(term)) ||
      (focusId !== null && focusNodeIds.has(node.id)) ||
      (isolatedLabel !== null && node.label === isolatedLabel)
    if (!wanted) continue
    const [x, y] = positionAt(buf, i)
    if (!onScreen(x, y)) continue
    candidates.push(i)
  }
  candidates.sort((a, b) => nodes[b].deg - nodes[a].deg)
  for (const i of candidates) {
    if (out.length >= LABEL_BUDGET) break
    push(i)
  }
  return out
}
