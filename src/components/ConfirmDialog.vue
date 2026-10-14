<script setup lang="ts">
import {
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogOverlay,
  AlertDialogPortal,
  AlertDialogRoot,
  AlertDialogTitle,
} from "reka-ui"

// a yes or no question. The parent hides it once it has the answer
export interface Question {
  title: string
  text: string
  confirm: string
}

defineProps<{ question?: Question }>()
const emit = defineEmits<{ answer: [yes: boolean] }>()
</script>

<template>
  <AlertDialogRoot
    :open="question !== undefined"
    @update:open="(open) => open || emit('answer', false)"
  >
    <AlertDialogPortal>
      <AlertDialogOverlay class="overlay" />
      <AlertDialogContent v-if="question" class="dialog">
        <AlertDialogTitle class="font-semibold">
          {{ question.title }}
        </AlertDialogTitle>
        <AlertDialogDescription class="mt-2 text-sm text-muted">
          {{ question.text }}
        </AlertDialogDescription>
        <div class="mt-5 flex justify-end gap-2">
          <button class="btn" @click="emit('answer', false)">Cancel</button>
          <button class="btn-danger" @click="emit('answer', true)">
            {{ question.confirm }}
          </button>
        </div>
      </AlertDialogContent>
    </AlertDialogPortal>
  </AlertDialogRoot>
</template>
