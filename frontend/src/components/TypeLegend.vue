<script setup lang="ts">
import { computed, ref } from 'vue'
import { cn } from '@nuvek/ui'
import { useGraph } from '../composables/useGraph'
import { colorFor } from '../color'
import { MOD_LABEL } from '../platform'
import * as hud from './hud'

const { counts, hidden, isHidden, toggleLabel, soloLabel, showAllLabels } = useGraph()

// Plain click toggles one type; ctrl/⌘-click isolates it (and isolating it again restores all).
// Both modifiers are accepted; `MOD_LABEL` decides only which one we *tell* the user to press,
// since ctrl-click is a right-click on macOS.
const onLegendClick = (e: MouseEvent, label: string) => {
  if (e.ctrlKey || e.metaKey) soloLabel(label)
  else toggleLabel(label)
}

// Only count labels that are both hidden and actually present in the current graph, so the
// collapsed summary can never claim filters that aren't doing anything.
const hiddenCount = computed(() => counts.value.filter(([l]) => hidden.value.has(l)).length)

// The legend is the tallest thing in the HUD, so it starts collapsed where vertical space is
// scarce — on a phone it otherwise runs straight into the search box.
const open = ref(!window.matchMedia('(max-width: 640px)').matches)
const onToggle = (e: Event) => {
  open.value = (e.target as HTMLDetailsElement).open
}
</script>

<template>
  <details :class="hud.section" :open="open" @toggle="onToggle">
    <summary :class="hud.summary">
      <span :class="hud.chevron" aria-hidden="true"></span>
      <h2 :class="hud.heading">Types</h2>
      <span class="text-sm text-fg-dim tabular-nums">{{ counts.length }}</span>
      <!-- Filters are invisible while collapsed; surface them so hidden types can't be forgotten.
           Kept a plain span — summary is itself a button, so it must not nest one. -->
      <span v-if="hiddenCount" class="text-xs text-warning tabular-nums">{{ hiddenCount }} off</span>
    </summary>
    <!-- The list scrolls inside the HUD rather than growing the panel off-screen. Filtering types
         changes its length constantly; the reserved gutter keeps rows from shifting sideways as
         the scrollbar comes and goes. -->
    <div class="mt-2 max-h-[min(46vh,340px)] overflow-y-auto [scrollbar-gutter:stable]">
      <button
        v-for="[label, ct] in counts"
        :key="label"
        type="button"
        :class="
          cn(
            'flex w-full cursor-pointer items-center gap-2 rounded-sm px-1 py-0.75 text-start select-none hover:bg-fg/5',
            isHidden(label) && 'opacity-35',
          )
        "
        :aria-pressed="!isHidden(label)"
        :title="`Click to show/hide · ${MOD_LABEL}-click to isolate this type`"
        @click="onLegendClick($event, label)"
      >
        <span class="size-2.5 flex-none rounded-full" :style="{ background: colorFor(label) }"></span>
        <span class="flex-1 truncate">{{ label }}</span>
        <span class="text-sm text-fg-dim tabular-nums">{{ ct }}</span>
      </button>
    </div>
    <p :class="hud.tip">
      {{ MOD_LABEL }}-click to isolate<template v-if="hiddenCount">
        ·
        <button type="button" :class="hud.inlineLink" @click="showAllLabels()">show all</button>
      </template>
    </p>
  </details>
</template>
