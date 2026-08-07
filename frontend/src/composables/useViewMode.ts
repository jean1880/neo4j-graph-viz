import { computed, ref } from 'vue'

/**
 * 2D or 3D, as a singleton store.
 *
 * The two modes differ in more than the camera: 3D runs the simulation in three dimensions and
 * renders through an orbit camera, so switching is a full re-settle rather than a view change.
 *
 * A note on why 2D remains the default. 3D looks better than it reads — at 25 000 nodes the
 * hairball becomes *solid*, and you lose both the ability to see through it and the stable
 * mental map that makes a persistent view worth having. Where 3D genuinely helps is a sparse
 * graph, which is exactly what search produces.
 */
export type Dimensions = 2 | 3

/**
 * Starting mode, from build-time config (`VITE_DEFAULT_VIEW_MODE=2|3`).
 *
 * Anything other than `3` means 2D, so a typo degrades to the safer default rather than to a
 * mode that is harder to read on a large graph.
 */
function defaultDimensions(): Dimensions {
  return String(import.meta.env.VITE_DEFAULT_VIEW_MODE ?? '').trim() === '3' ? 3 : 2
}

const dimensions = ref<Dimensions>(defaultDimensions())

const is3D = computed(() => dimensions.value === 3)

function toggle() {
  dimensions.value = dimensions.value === 2 ? 3 : 2
}

function set(d: Dimensions) {
  dimensions.value = d
}

/** The raw ref, for modules that need the value without taking the whole store. */
export { dimensions }

export function useViewMode() {
  return { dimensions, is3D, toggle, set }
}
