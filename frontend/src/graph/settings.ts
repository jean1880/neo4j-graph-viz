import type { Dimensions } from '../composables/useViewMode'

/**
 * User-tunable layout and display settings — the Obsidian-style "Display / Forces" knobs.
 *
 * These were compile-time constants, tuned for one graph shape. That tuning is still the
 * *default*, but a 200-node search result and a 25 000-node hairball genuinely want different
 * spacing, and no single constant serves both. So the constants became defaults and the defaults
 * became adjustable.
 *
 * **Kept per dimensionality on purpose.** 2D and 3D are different layouts, not a projection
 * (see CLAUDE.md), and their tuned values are not merely scaled versions of each other — a volume
 * layout needs containment that makes a plane layout balloon. One shared set of sliders would mean
 * every mode switch undoes the tuning you just did for the other mode.
 */
export interface LayoutSettings {
  /** Multiplier on the mode's node-radius rule. 1 = the tuned default. Not purely cosmetic:
   *  radius feeds the collision force, so bigger nodes have to be given room to sit in — the
   *  canvas re-settles after this changes. */
  nodeSize: number
  /** Edge stroke width, in pixels. The one genuinely cosmetic setting: it repaints and nothing
   *  moves. */
  linkThickness: number
  /** Rest length of a link, as a fraction of the **server** layout's median edge. The primary
   *  "how far apart are things" knob. */
  linkDistance: number
  /** Multiplier on the charge (repulsion) magnitude. 1 = the tuned default. */
  repel: number
  /** How hard a link pulls its endpoints toward the rest length, 0–1. */
  linkForce: number
  /** Pull toward the origin. Zero in 2D by default; a volume layout needs a little or its
   *  disconnected components drift apart forever. */
  centreForce: number
}

/** The tuned constants this viewer shipped with — the value every slider resets to. */
export const DEFAULT_SETTINGS: Record<Dimensions, LayoutSettings> = {
  2: {
    nodeSize: 1,
    linkThickness: 1.3,
    linkDistance: 1,
    repel: 1,
    linkForce: 0.35,
    centreForce: 0,
  },
  3: {
    nodeSize: 1,
    linkThickness: 1.8,
    linkDistance: 0.85,
    repel: 1,
    linkForce: 0.45,
    centreForce: 0.02,
  },
} as const

export type SettingKey = keyof LayoutSettings

/** Slider metadata, so the panel is a loop rather than six hand-written rows. */
export interface SettingSpec {
  key: SettingKey
  label: string
  min: number
  max: number
  step: number
  /** How the value reads to a human. Multipliers are shown as `×`, the rest as plain numbers. */
  unit?: string
  hint: string
  /** Which section of the panel the slider sits in. Note this is *not* the same split as
   *  "repaints" vs "re-settles" — node size is a display control that still re-settles, because
   *  a radius the collision force has not seen is a radius that overlaps. */
  group: 'display' | 'forces'
}

export const SETTING_SPECS: readonly SettingSpec[] = [
  {
    key: 'nodeSize',
    label: 'Node size',
    min: 0.2,
    max: 4,
    step: 0.05,
    unit: '×',
    hint: 'Node radius, relative to the tuned default',
    group: 'display',
  },
  {
    key: 'linkThickness',
    label: 'Link thickness',
    min: 0.4,
    max: 5,
    step: 0.1,
    unit: 'px',
    hint: 'Edge stroke width in pixels',
    group: 'display',
  },
  {
    key: 'linkDistance',
    label: 'Link distance',
    min: 0.2,
    max: 5,
    step: 0.05,
    unit: '×',
    hint: 'Rest length of a link, as a fraction of the median edge',
    group: 'forces',
  },
  {
    key: 'repel',
    label: 'Repel force',
    min: 0.1,
    max: 6,
    step: 0.1,
    unit: '×',
    hint: 'How hard nodes push each other apart',
    group: 'forces',
  },
  {
    key: 'linkForce',
    label: 'Link force',
    min: 0,
    max: 1,
    step: 0.05,
    hint: 'How hard a link pulls its endpoints together',
    group: 'forces',
  },
  {
    key: 'centreForce',
    label: 'Centre force',
    min: 0,
    max: 0.15,
    step: 0.005,
    hint: 'Pull toward the origin — keeps disconnected pieces from drifting away',
    group: 'forces',
  },
] as const

/** Clamp an arbitrary (possibly restored-from-storage, possibly hand-edited) value into range. */
export function clampSettings(raw: Partial<LayoutSettings>, d: Dimensions): LayoutSettings {
  const out = { ...DEFAULT_SETTINGS[d] }
  for (const spec of SETTING_SPECS) {
    const v = raw[spec.key]
    if (typeof v === 'number' && Number.isFinite(v)) {
      out[spec.key] = Math.min(spec.max, Math.max(spec.min, v))
    }
  }
  return out
}
