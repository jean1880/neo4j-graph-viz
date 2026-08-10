import { computed, reactive, watch } from 'vue'
import {
  clampSettings,
  DEFAULT_SETTINGS,
  type LayoutSettings,
  type SettingKey,
} from '../graph/settings'
import { dimensions, type Dimensions } from './useViewMode'

/**
 * The layout/display sliders, as a singleton store — one owner, so the panel and the canvas can
 * never disagree about what is set.
 *
 * Persisted to `localStorage`: tuning a graph is a per-user, per-graph act, and losing it on
 * every reload would make the sliders not worth using. Persistence is best-effort — a browser
 * with storage blocked (private mode, a locked-down profile) still gets working sliders, just not
 * sticky ones.
 */
const STORAGE_KEY = 'graph-viz.layout-settings.v1'

const store = reactive<Record<Dimensions, LayoutSettings>>({
  2: { ...DEFAULT_SETTINGS[2] },
  3: { ...DEFAULT_SETTINGS[3] },
})

function restore() {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    if (!raw) return
    const parsed = JSON.parse(raw) as Partial<Record<Dimensions, Partial<LayoutSettings>>>
    // Clamped, not trusted: stored values outlive the slider ranges that produced them, and an
    // out-of-range charge is the difference between a layout and a graph flung off-screen.
    store[2] = clampSettings(parsed[2] ?? {}, 2)
    store[3] = clampSettings(parsed[3] ?? {}, 3)
  } catch {
    /* Corrupt or unreadable storage: the defaults are already in place. */
  }
}
restore()

watch(
  store,
  (value) => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value))
    } catch {
      /* Storage unavailable or full — the session still works, it just will not be remembered. */
    }
  },
  { deep: true },
)

/** The settings for whichever mode is on screen. Editing this edits that mode alone. */
const settings = computed<LayoutSettings>(() => store[dimensions.value])

function set(key: SettingKey, value: number) {
  store[dimensions.value][key] = value
}

/** Restore the shipped tuning — for the current mode only, so resetting 3D leaves 2D alone. */
function reset() {
  store[dimensions.value] = { ...DEFAULT_SETTINGS[dimensions.value] }
}

const isDefault = computed(() => {
  const current = store[dimensions.value]
  const base = DEFAULT_SETTINGS[dimensions.value]
  return (Object.keys(base) as SettingKey[]).every((k) => current[k] === base[k])
})

export function useLayoutSettings() {
  return { settings, set, reset, isDefault }
}

/** The raw computed, for modules that want the values without the whole store. */
export { settings }
