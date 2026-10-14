<script setup lang="ts">
import { watch } from "vue"
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui"
import type { ModView } from "../api"
import { byName, byProblems, useListing, type Sort } from "../listing"
import ModIcon from "./ModIcon.vue"
import ModNotices from "./ModNotices.vue"
import SortPicker from "./SortPicker.vue"

const props = defineProps<{
  // every mod in the collection
  mods: ModView[]
  // ids of the mods already in the profile being edited
  used: string[]
  profile: string
}>()

const open = defineModel<boolean>("open", { required: true })
const emit = defineEmits<{
  add: [id: string]
  delete: [mod: ModView]
  import: []
}>()

// a collection has no load order and nothing is turned off in it
const sorts = {
  name: byName,
  used: {
    label: "In profile",
    key: ({ mod }) => (props.used.includes(mod.id) ? 0 : 1),
  },
  problems: byProblems,
} satisfies Record<string, Sort>
const { query, sort, reversed, shown } = useListing(
  () => props.mods,
  sorts,
  "name",
)

watch(open, (isOpen) => {
  if (isOpen) query.value = ""
})
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="overlay" />
      <DialogContent class="dialog flex max-h-[85vh] w-160 flex-col">
        <div class="flex items-start gap-3">
          <div class="flex-1">
            <DialogTitle class="font-semibold">Your mods</DialogTitle>
            <DialogDescription class="text-sm text-muted">
              All imported mods. Delete them from your collection, or add them
              to
              <span class="font-semibold">{{ profile }}</span
              >.
            </DialogDescription>
          </div>
          <DialogClose class="btn">Close</DialogClose>
        </div>

        <div class="mt-4 flex gap-2">
          <SortPicker
            v-model:sort="sort"
            v-model:reversed="reversed"
            :sorts="sorts"
          />
          <input
            v-model="query"
            class="field flex-1"
            type="search"
            placeholder="Search your mods"
            aria-label="Search your mods"
          />
          <button class="btn-primary" @click="emit('import')">
            Import .zip
          </button>
        </div>

        <slot />

        <p v-if="!mods.length" class="py-10 text-center text-sm text-muted">
          Nothing imported yet. Import a mod .zip, or drop one on the window.
        </p>
        <p
          v-else-if="!shown.length"
          class="py-10 text-center text-sm text-muted"
        >
          No mod matches that search.
        </p>
        <ul v-else class="mt-3 flex flex-col gap-1.5 overflow-auto">
          <li
            v-for="{ mod } in shown"
            :key="mod.id"
            class="flex items-center gap-3 rounded-md border border-line px-3 py-2"
          >
            <ModIcon :mod="mod" />
            <div class="min-w-0 flex-1">
              <p class="flex flex-wrap items-baseline gap-x-2">
                <span class="font-semibold">{{ mod.name }}</span>
                <span class="text-sm text-muted">{{ mod.version }}</span>
                <span v-if="mod.authors.length" class="text-sm text-muted">
                  by {{ mod.authors.join(", ") }}
                </span>
              </p>
              <p v-if="mod.description" class="truncate text-sm text-muted">
                {{ mod.description }}
              </p>
              <ModNotices :mod="mod" class="mt-1" />
            </div>
            <button
              class="btn"
              :disabled="used.includes(mod.id)"
              @click="emit('add', mod.id)"
            >
              {{ used.includes(mod.id) ? "In profile" : "Add" }}
            </button>
            <button
              class="btn-danger"
              :aria-label="`Delete ${mod.name}`"
              @click="emit('delete', mod)"
            >
              Delete
            </button>
          </li>
        </ul>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
