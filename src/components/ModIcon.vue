<script setup lang="ts">
import type { ModView } from "../api"

defineProps<{ mod: ModView }>()
</script>

<template>
  <img
    v-if="mod.icon"
    :src="mod.icon"
    alt=""
    class="size-10 shrink-0 rounded-md border border-line object-cover"
  />
  <span
    v-else
    class="flex size-10 shrink-0 items-center justify-center rounded-md border border-line bg-canvas font-semibold text-muted"
    aria-hidden="true"
  >
    {{ mod.name.charAt(0).toUpperCase() }}
  </span>
</template>
