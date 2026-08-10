<script setup lang="ts">
import { computed, ref } from 'vue'
import { useLayoutSettings } from '../composables/useLayoutSettings'
import { useViewMode } from '../composables/useViewMode'
import { SETTING_SPECS, type SettingSpec } from '../graph/settings'

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

const display = SETTING_SPECS.filter((s) => s.group === 'display')
const forces = SETTING_SPECS.filter((s) => s.group === 'forces')

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
  <details class="settings" :open="open" @toggle="onToggle">
    <summary>
      <span class="chev" aria-hidden="true"></span>
      <h2>Layout</h2>
      <span class="mode" :title="`These sliders apply to ${modeName} only`">{{ modeName }}</span>
      <!-- Kept a plain span: summary is itself a button and must not nest one. -->
      <span v-if="!isDefault" class="tweaked">edited</span>
    </summary>

    <div class="grp">
      <h3>Display</h3>
      <label v-for="spec in display" :key="spec.key" class="row" :title="spec.hint">
        <span class="nm">{{ spec.label }}</span>
        <span class="val">{{ format(spec, settings[spec.key]) }}</span>
        <input
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

    <div class="grp">
      <h3>Forces</h3>
      <label v-for="spec in forces" :key="spec.key" class="row" :title="spec.hint">
        <span class="nm">{{ spec.label }}</span>
        <span class="val">{{ format(spec, settings[spec.key]) }}</span>
        <input
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

    <p class="tip">
      Saved per browser<template v-if="!isDefault">
        ·
        <button type="button" class="reset" @click="reset()">reset {{ modeName }}</button>
      </template>
    </p>
  </details>
</template>

<style scoped>
.settings {
  margin-top: var(--space-3);
  border-top: 1px solid var(--border-alt);
  padding-top: var(--space-2);
}
summary {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  cursor: pointer;
  padding: 2px 0;
  border-radius: var(--radius-sm);
  user-select: none;
  /* Replaced by the chevron below. */
  list-style: none;
}
summary::-webkit-details-marker {
  display: none;
}
summary:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.chev {
  flex: 0 0 auto;
  inline-size: 0;
  block-size: 0;
  border-inline-start: 5px solid currentColor;
  border-block: 4px solid transparent;
  color: var(--text-dim);
  transition: rotate 160ms ease;
}
.settings[open] .chev {
  rotate: 90deg;
}
h2 {
  margin: 0;
  flex: 1;
  font-size: var(--text-sm);
  text-transform: uppercase;
  letter-spacing: 0.8px;
  color: var(--text-muted);
  font-weight: 600;
}
.mode {
  font-size: var(--text-xs);
  color: var(--text-dim);
  border: 1px solid var(--border-alt);
  border-radius: 20px;
  padding: 0 5px;
}
.tweaked {
  font-size: var(--text-xs);
  color: var(--primary);
}
.grp {
  margin-top: var(--space-2);
}
h3 {
  margin: 0 0 2px;
  font-size: var(--text-xs);
  text-transform: uppercase;
  letter-spacing: 0.6px;
  color: var(--text-dim);
  font-weight: 600;
}
.row {
  display: grid;
  /* Name and value share a line; the slider spans the full width beneath them, which is the only
     way a 230px panel gives the track enough travel to be usable. */
  grid-template-columns: 1fr auto;
  align-items: center;
  gap: 0 var(--space-2);
  padding: 2px 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
  cursor: pointer;
}
.nm {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.val {
  font-size: var(--text-xs);
  color: var(--text-dim);
  font-variant-numeric: tabular-nums;
}
.row input {
  grid-column: 1 / -1;
  inline-size: 100%;
  min-inline-size: 0;
  margin: 0;
  accent-color: var(--primary);
}
.tip {
  margin: var(--space-2) 0 0;
  font-size: var(--text-xs);
  color: var(--text-dim);
}
.reset {
  border: none;
  background: transparent;
  padding: 0;
  color: var(--primary);
  font: inherit;
  cursor: pointer;
  text-decoration: underline;
}
</style>
