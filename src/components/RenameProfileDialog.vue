<script setup lang="ts">
import { ref, watch } from "vue"
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui"

// the name the profile has now
const props = defineProps<{ name: string }>()

const open = defineModel<boolean>("open", { required: true })
const emit = defineEmits<{ rename: [to: string] }>()
const to = ref("")

watch(open, (shown) => {
  if (shown) to.value = props.name
})

function submit() {
  const trimmed = to.value.trim()
  if (!trimmed) return
  if (trimmed !== props.name) emit("rename", trimmed)
  open.value = false
}
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="overlay" />
      <DialogContent class="dialog">
        <DialogTitle class="font-semibold">Rename profile</DialogTitle>
        <DialogDescription class="mt-2 text-sm text-muted">
          A new name for {{ name }}. Its mods stay as they are.
        </DialogDescription>
        <form class="mt-4 flex flex-col gap-3" @submit.prevent="submit">
          <label class="flex flex-col gap-1 text-sm">
            Name
            <input v-model="to" class="field" maxlength="40" />
          </label>
          <div class="mt-2 flex justify-end gap-2">
            <DialogClose class="btn" type="button">Cancel</DialogClose>
            <button class="btn-primary" type="submit" :disabled="!to.trim()">
              Rename
            </button>
          </div>
        </form>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
