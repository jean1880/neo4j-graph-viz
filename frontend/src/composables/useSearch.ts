import { computed, ref } from 'vue'
import { apiUrl } from '../api'
import type { SearchResponse } from '../types'

/**
 * Search state, as a singleton store.
 *
 * Search runs on the *server*: node properties no longer travel with the graph payload, and the
 * interesting part — deciding what counts as related — is a graph traversal over the adjacency
 * the backend already holds. The result **removes** unrelated nodes rather than dimming them,
 * and carries fresh coordinates for the survivors.
 *
 * This module deliberately knows nothing about the graph store or the canvas. `useGraph`
 * observes `searchResult` to reconcile its own selection, which keeps the dependency pointing
 * one way and avoids a cycle between the two composables.
 */
const query = ref('')
const searchResult = ref<SearchResponse | null>(null)
const searching = ref(false)
const searchError = ref<string | null>(null)
/** How far relatedness spreads from a match. One knob: 0 = matches only, 1 = as far as decay
 *  allows. Match strength and graph distance are collapsed into a single number server-side, so
 *  this is genuinely the only control needed. */
const breadth = ref(0.6)

let inFlight: AbortController | null = null

/** Ids that survived the current search, or `null` when no search is active. */
const searchVisible = computed<ReadonlySet<string> | null>(() => {
  const r = searchResult.value
  return r ? new Set(r.visible.map((v) => v.id)) : null
})

/**
 * Run a search against the backend. Cancels any in-flight request first: searches are issued
 * per keystroke-batch and responses are not guaranteed to arrive in order, so without this a
 * slow early request could land after a fast later one and show the wrong subgraph.
 */
async function runSearch(q: string) {
  inFlight?.abort()
  inFlight = null
  const term = q.trim()
  if (!term) {
    searchResult.value = null
    searchError.value = null
    searching.value = false
    return
  }

  const ctrl = new AbortController()
  inFlight = ctrl
  searching.value = true
  searchError.value = null
  try {
    const res = await fetch(
      apiUrl('api/search', { q: term, breadth: String(breadth.value) }),
      { signal: ctrl.signal },
    )
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const body = (await res.json()) as SearchResponse
    if (ctrl.signal.aborted) return
    searchResult.value = body
  } catch (e) {
    if (ctrl.signal.aborted) return
    searchError.value = e instanceof Error ? e.message : String(e)
    searchResult.value = null
  } finally {
    if (inFlight === ctrl) {
      inFlight = null
      searching.value = false
    }
  }
}

function clearSearch() {
  query.value = ''
  void runSearch('')
}

export function useSearch() {
  return {
    query,
    searchResult,
    searchVisible,
    searching,
    searchError,
    breadth,
    runSearch,
    clearSearch,
  }
}

/** The survivor set, for modules that need it without taking the whole store. */
export { searchVisible }
