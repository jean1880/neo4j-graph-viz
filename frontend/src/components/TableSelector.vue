<script setup lang="ts">
import { cn } from '@nuvek/ui'
import { useGraph } from '../composables/useGraph'

const { availableGroups, selectedGroup, setGroup } = useGraph()

const DESCRIPTIONS: Record<string, string> = {
  COTGE: 'Collapse of the Great Empire (Victoria 3 Mod)',
  HL: 'Homelab Infrastructure',
  MH: 'Media Homelab',
}

function titleFor(grp: string): string {
  return DESCRIPTIONS[grp] || `Table: ${grp}`
}

const pill = (active: boolean) =>
  cn(
    'flex-1 min-h-6.5 cursor-pointer rounded-sm px-2 py-1 text-center text-xs font-semibold tracking-wide transition-colors focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-primary select-none',
    active
      ? 'bg-primary text-bg hover:bg-primary hover:text-bg shadow-xs'
      : 'text-fg-dim hover:bg-fg/7 hover:text-fg',
  )
</script>

<template>
  <div v-if="availableGroups.length > 1" class="w-full">
    <!-- Segmented control for 4 or fewer tables, ensuring single exclusive inspection -->
    <div
      v-if="availableGroups.length <= 4"
      class="flex w-full items-center gap-0.5 rounded-sm border border-border-strong bg-surface/80 p-0.5"
      role="group"
      aria-label="Table grouping selector"
    >
      <button
        v-for="grp in availableGroups"
        :key="grp"
        type="button"
        :class="pill(selectedGroup === grp)"
        :aria-pressed="selectedGroup === grp"
        :title="titleFor(grp)"
        @click="setGroup(grp)"
      >
        {{ grp }}
      </button>
    </div>

    <!-- Dropdown selector if more than 4 tables exist -->
    <div v-else class="flex w-full flex-col gap-1">
      <label for="table-select" class="text-xs font-medium text-fg-dim">Table</label>
      <select
        id="table-select"
        :value="selectedGroup"
        class="w-full rounded-sm border border-border-strong bg-surface px-2 py-1.5 text-xs text-fg outline-none focus-visible:border-primary"
        aria-label="Select active table"
        @change="setGroup(($event.target as HTMLSelectElement).value)"
      >
        <option v-for="grp in availableGroups" :key="grp" :value="grp" :title="titleFor(grp)">
          {{ grp }} — {{ DESCRIPTIONS[grp] || 'Graph Table' }}
        </option>
      </select>
    </div>
  </div>
</template>
