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

defineProps<{ profiles: string[] }>()

const open = defineModel<boolean>("open", { required: true })
// base is the profile to copy, or null to start empty
const emit = defineEmits<{ create: [name: string, base: string | null] }>()
const name = ref("")
const base = ref("")

watch(open, (shown) => {
  if (shown) {
    name.value = ""
    base.value = ""
  }
})

function submit() {
  const trimmed = name.value.trim()
  if (!trimmed) return
  emit("create", trimmed, base.value || null)
  open.value = false
}
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="overlay" />
      <DialogContent class="dialog">
        <DialogTitle class="font-semibold">New profile</DialogTitle>
        <DialogDescription class="mt-2 text-sm text-muted">
          A profile is a set of mods and their order. Mods you have already
          added can be used in any profile.
        </DialogDescription>
        <form class="mt-4 flex flex-col gap-3" @submit.prevent="submit">
          <label class="flex flex-col gap-1 text-sm">
            Name
            <input v-model="name" class="field" maxlength="40" />
          </label>
          <label class="flex flex-col gap-1 text-sm">
            Start from
            <select v-model="base" class="field">
              <option value="">Nothing, an empty profile</option>
              <option v-for="profile in profiles" :key="profile">
                {{ profile }}
              </option>
            </select>
          </label>
          <div class="mt-2 flex justify-end gap-2">
            <DialogClose class="btn" type="button">Cancel</DialogClose>
            <button class="btn-primary" type="submit" :disabled="!name.trim()">
              Create
            </button>
          </div>
        </form>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
