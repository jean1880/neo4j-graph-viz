<script setup lang="ts">
import { computed } from 'vue'
import { useGraph } from '../composables/useGraph'
import { colorFor } from '../color'
import type { GraphNode } from '../types'

const { selectedNode, clearSelection, neighboursOf, select } = useGraph()

const neighbours = computed(() =>
  selectedNode.value ? neighboursOf(selectedNode.value) : [],
)

const relSummary = computed(() => {
  const byType = new Map<string, number>()
  for (const n of neighbours.value) byType.set(n.type, (byType.get(n.type) ?? 0) + 1)
  return [...byType.entries()].sort((a, b) => b[1] - a[1])
})

const propRows = computed(() =>
  selectedNode.value
    ? Object.entries(selectedNode.value.props).filter(([k]) => k !== 'name')
    : [],
)

const chips = computed(() => neighbours.value.slice(0, 60))
const moreCount = computed(() => Math.max(0, neighbours.value.length - 60))

function goto(node: GraphNode | undefined) {
  if (node) select(node)
}
</script>

<template>
  <div v-if="selectedNode" class="nv-graph-panel detail">
    <div class="dhead">
      <span class="dot" :style="{ background: colorFor(selectedNode.label) }"></span>
      <h3>{{ selectedNode.name }}</h3>
      <button type="button" class="close" aria-label="Close node details" @click="clearSelection">
        ×
      </button>
    </div>
    <div class="meta">
      {{ selectedNode.label }} · {{ selectedNode.group }} ·
      {{ selectedNode.deg }} connection{{ selectedNode.deg === 1 ? '' : 's' }}
    </div>

    <div class="sec">Properties</div>
    <table>
      <tbody>
        <tr v-if="propRows.length === 0">
          <td class="v muted">no properties</td>
        </tr>
        <tr v-for="[k, v] in propRows" :key="k">
          <td class="k">{{ k }}</td>
          <td class="v">{{ v }}</td>
        </tr>
      </tbody>
    </table>

    <div class="sec">Connections</div>
    <div v-if="relSummary.length" class="rels">
      <template v-for="([t, c], i) in relSummary" :key="t"
        ><span v-if="i > 0"> · </span>{{ t }} <span class="muted">{{ c }}</span></template
      >
    </div>
    <div>
      <button
        v-for="(nb, i) in chips"
        :key="i"
        type="button"
        class="nb"
        :title="`${nb.dir} ${nb.type}`"
        @click="goto(nb.node)"
      >
        {{ nb.node?.name ?? '?' }}
      </button>
      <div v-if="moreCount" class="muted more">+{{ moreCount }} more…</div>
    </div>
  </div>
</template>

<style scoped>
.detail {
  bottom: var(--space-4);
  right: var(--space-4);
  width: 300px;
  max-height: 62vh;
  overflow-y: auto;
}
.dhead {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  margin-bottom: 4px;
}
.dot {
  width: 11px;
  height: 11px;
  border-radius: 50%;
  flex: 0 0 auto;
  margin-top: 3px;
}
h3 {
  margin: 0;
  font-size: var(--text-base);
  font-weight: 650;
  flex: 1;
  word-break: break-word;
}
.close {
  cursor: pointer;
  color: var(--text-muted);
  font-size: 17px;
  line-height: 1;
  padding: 0 4px;
  border: none;
  background: transparent;
  border-radius: 5px;
  flex: 0 0 auto;
}
.close:hover {
  background: color-mix(in srgb, var(--text) 8%, transparent);
  color: var(--text);
}
.meta {
  color: var(--text-muted);
  font-size: var(--text-xs);
  margin-bottom: 6px;
}
.sec {
  text-transform: uppercase;
  letter-spacing: 0.8px;
  font-size: 10px;
  color: var(--text-dim);
  margin: 13px 0 6px;
}
table {
  width: 100%;
  border-collapse: collapse;
}
td {
  padding: 3px 0;
  vertical-align: top;
  font-size: var(--text-sm);
}
td.k {
  color: var(--text-muted);
  padding-right: 10px;
  white-space: nowrap;
  width: 1%;
}
td.v {
  color: var(--text);
  word-break: break-word;
}
.muted {
  color: var(--text-dim);
}
.rels {
  font-size: var(--text-xs);
  margin-bottom: var(--space-2);
}
.nb {
  display: inline-block;
  margin: 2px 4px 2px 0;
  padding: 2px 8px;
  border-radius: 20px;
  background: var(--surface);
  border: 1px solid var(--border-alt);
  color: var(--text-muted);
  font-family: inherit;
  line-height: 1.4;
  cursor: pointer;
  font-size: var(--text-xs);
}
.nb:hover {
  border-color: var(--primary);
  color: var(--text);
}
.more {
  margin-top: 6px;
}
</style>
