<script setup lang="ts">
import type { ModView } from "../api"

defineProps<{ mod: ModView }>()
</script>

<template>
  <ul
    v-if="mod.errors.length || mod.warnings.length"
    class="flex flex-col gap-1.5 text-sm"
  >
    <li
      v-for="(notice, index) in [...mod.errors, ...mod.warnings]"
      :key="index"
      :class="index < mod.errors.length ? 'text-danger' : 'text-warn'"
    >
      <p class="font-medium">{{ notice.title }}</p>
      <dl
        v-if="notice.details.length"
        class="grid grid-cols-[auto_1fr] gap-x-3 text-xs"
      >
        <template v-for="[label, value] in notice.details" :key="label">
          <dt class="text-ink">{{ label }}</dt>
          <dd class="break-all text-muted select-text">{{ value }}</dd>
        </template>
      </dl>
    </li>
  </ul>
</template>
