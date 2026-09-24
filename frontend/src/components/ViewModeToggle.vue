<script setup lang="ts">
import { cn } from '@nuvek/ui'
import { useViewMode } from '../composables/useViewMode'

const { dimensions, set } = useViewMode()

const mode = (on: boolean) =>
  cn(
    'min-h-6.5 cursor-pointer rounded-sm px-3.5 py-1.25 text-sm leading-normal font-semibold tracking-wide text-fg-dim hover:bg-fg/7 hover:text-fg focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-primary',
    on && 'bg-primary text-bg hover:bg-primary hover:text-bg',
  )
</script>

<template>
  <!-- A segmented control rather than a switch: with two named states there is no ambiguity
       about which one is active, which a bare toggle always has. -->
  <div class="graph-panel top-4 right-4 z-15 flex gap-0.5 p-0.75" role="group" aria-label="View mode">
    <button
      type="button"
      :class="mode(dimensions === 2)"
      :aria-pressed="dimensions === 2"
      title="Flat view — best for reading structure"
      @click="set(2)"
    >
      2D
    </button>
    <button
      type="button"
      :class="mode(dimensions === 3)"
      :aria-pressed="dimensions === 3"
      title="Orbit view — drag to rotate. Clearer on small graphs than on the full map."
      @click="set(3)"
    >
      3D
    </button>
  </div>
</template>
