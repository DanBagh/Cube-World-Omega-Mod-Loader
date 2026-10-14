<script setup lang="ts" generic="K extends string">
// the ways the list can be ordered, by key
defineProps<{ sorts: Record<string, { label: string }> }>()

// one of the keys of `sorts`
const sort = defineModel<K>("sort", { required: true })
const reversed = defineModel<boolean>("reversed", { required: true })
</script>

<template>
  <div class="flex items-center gap-2">
    <label class="flex items-center gap-2 text-sm text-muted">
      Sort
      <select v-model="sort" class="field text-ink">
        <option v-for="(way, key) in sorts" :key="key" :value="key">
          {{ way.label }}
        </option>
      </select>
    </label>
    <button
      class="btn"
      :class="{ 'border-accent text-accent': reversed }"
      :aria-pressed="reversed"
      aria-label="Reverse the order"
      title="Reverse the order"
      @click="reversed = !reversed"
    >
      {{ reversed ? "▲" : "▼" }}
    </button>
  </div>
</template>
