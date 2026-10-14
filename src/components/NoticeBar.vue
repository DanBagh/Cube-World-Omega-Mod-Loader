<script setup lang="ts">
defineProps<{ error: string; note: string }>()
</script>

<template>
  <div v-if="error || note" class="flex flex-col gap-2">
    <p
      v-if="error"
      class="rounded-md border border-danger px-3 py-2 text-sm text-danger select-text"
    >
      {{ error }}
    </p>
    <p
      v-if="note"
      class="rounded-md border border-line px-3 py-2 text-sm text-muted"
    >
      {{ note }}
    </p>
  </div>
</template>
