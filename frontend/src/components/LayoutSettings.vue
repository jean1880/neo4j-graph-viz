<script setup lang="ts">
import { computed, ref } from 'vue'
import { useLayoutSettings } from '../composables/useLayoutSettings'
import { useViewMode } from '../composables/useViewMode'
import { SETTING_SPECS, type SettingSpec } from '../graph/settings'
import * as hud from './hud'

/**
 * The Obsidian-style Display / Forces sliders.
 *
 * Two groups: **display** is what a node or edge looks like, **forces** is what holds the layout
 * in equilibrium. That split is about the user's intent, not about cost — node size lives under
 * display but still re-settles, because radius feeds the collision force and a size the layout
 * has not seen is a size that overlaps its neighbours.
 *
 * The sliders edit whichever mode is on screen — 2D and 3D keep separate values, because they
 * are separate layouts (see `graph/settings.ts`). The heading says which one you are editing, so
 * "I tuned this and it reverted" can never be a mystery.
 */
const { settings, set, reset, isDefault } = useLayoutSettings()
const { dimensions } = useViewMode()

const groups = [
  { title: 'Display', specs: SETTING_SPECS.filter((s) => s.group === 'display') },
  { title: 'Forces', specs: SETTING_SPECS.filter((s) => s.group === 'forces') },
]

/** Enough precision to see a step move, without a slider reading `1.7000000000000002`. */
function format(spec: SettingSpec, value: number): string {
  const decimals = spec.step < 0.01 ? 3 : spec.step < 0.1 ? 2 : 1
  return `${value.toFixed(decimals)}${spec.unit ?? ''}`
}

const modeName = computed(() => (dimensions.value === 3 ? '3D' : '2D'))

// Collapsed by default: this is the panel you open when the defaults are not working for the
// graph in front of you, not something to scroll past every session.
const open = ref(false)
const onToggle = (e: Event) => {
  open.value = (e.target as HTMLDetailsElement).open
}
</script>

<template>
  <details :class="hud.section" :open="open" @toggle="onToggle">
    <summary :class="hud.summary">
      <span :class="hud.chevron" aria-hidden="true"></span>
      <h2 :class="hud.heading">Layout</h2>
      <span
        class="rounded-full border border-border-strong px-1.25 text-xs text-fg-dim"
        :title="`These sliders apply to ${modeName} only`"
      >{{ modeName }}</span>
      <!-- Kept a plain span: summary is itself a button and must not nest one. -->
      <span v-if="!isDefault" class="text-xs text-primary">edited</span>
    </summary>

    <div v-for="g in groups" :key="g.title" class="mt-2">
      <h3 class="mb-0.5 text-xs font-semibold tracking-wider text-fg-dim uppercase">{{ g.title }}</h3>
      <!-- Name and value share a line; the slider spans the full width beneath them, which is the
           only way a 230px panel gives the track enough travel to be usable. -->
      <label
        v-for="spec in g.specs"
        :key="spec.key"
        class="grid cursor-pointer grid-cols-[1fr_auto] items-center gap-x-2 py-0.5 text-sm text-fg-muted"
        :title="spec.hint"
      >
        <span class="truncate">{{ spec.label }}</span>
        <span class="text-xs text-fg-dim tabular-nums">{{ format(spec, settings[spec.key]) }}</span>
        <input
          class="col-span-full m-0 w-full min-w-0 accent-primary"
          type="range"
          :min="spec.min"
          :max="spec.max"
          :step="spec.step"
          :value="settings[spec.key]"
          :aria-label="`${spec.label} — ${spec.hint}`"
          @input="set(spec.key, Number(($event.target as HTMLInputElement).value))"
        />
      </label>
    </div>

    <p :class="hud.tip">
      Saved per browser<template v-if="!isDefault">
        ·
        <button type="button" :class="hud.inlineLink" @click="reset()">reset {{ modeName }}</button>
      </template>
    </p>
  </details>
</template>
