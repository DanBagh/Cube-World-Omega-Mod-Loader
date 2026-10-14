<script setup lang="ts">
import { computed } from "vue"
import { SwitchRoot, SwitchThumb } from "reka-ui"
import type { ModView } from "../api"
import ModIcon from "./ModIcon.vue"
import ModNotices from "./ModNotices.vue"

const props = defineProps<{
  mod: ModView
  first: boolean
  last: boolean
  // false while the list is searched or sorted another way, when moving would be ambiguous
  sortable: boolean
}>()

const pinned = "Clear the search and sort by load order to move mods"

const emit = defineEmits<{
  toggle: []
  up: []
  down: []
  remove: []
}>()

// What a mod changes, in a player's words. Each kind of content the backend counts falls
// under one tag, and reads as [one, many] in that tag's tooltip
const groups: [string, Record<string, [string, string]>][] = [
  [
    "Text",
    {
      text: ["text change", "text changes"],
      tooltips: ["tooltip line", "tooltip lines"],
    },
  ],
  [
    "Models",
    {
      models: ["model replaced", "models replaced"],
      groundModels: ["model for item piles", "models for item piles"],
      glow: ["item that glows", "items that glow"],
    },
  ],
  [
    "Sounds",
    {
      sounds: ["sound", "sounds"],
      pickupSounds: ["pickup sound", "pickup sounds"],
      soundPitch: ["pitch change", "pitch changes"],
    },
  ],
  ["Files", { files: ["other file", "other files"] }],
]

const tags = computed(() => {
  const found = groups.map(([label, kinds]) => ({
    label,
    parts: props.mod.contents
      .filter((content) => content.label in kinds)
      .map(({ label, count }) => `${count} ${kinds[label][+(count !== 1)]}`),
  }))
  // a kind this version has no words for yet
  const known = groups.flatMap(([, kinds]) => Object.keys(kinds))
  found.push({
    label: "Other",
    parts: props.mod.contents
      .filter((content) => !known.includes(content.label))
      .map(({ label, count }) => `${count} ${label}`),
  })
  return found
    .filter((tag) => tag.parts.length)
    .map((tag) => ({ label: tag.label, detail: tag.parts.join(", ") }))
})
</script>

<template>
  <li
    class="flex gap-3 rounded-md border border-line bg-surface px-3 py-3"
    :class="{ 'opacity-60': !mod.enabled }"
  >
    <div
      class="flex flex-col items-center gap-1"
      :title="sortable ? undefined : pinned"
    >
      <button
        class="btn px-1.5 py-0 leading-5"
        :disabled="first || !sortable"
        :aria-label="`Move ${mod.name} up`"
        @click="emit('up')"
      >
        ▲
      </button>
      <span class="text-sm text-muted" title="Place in the load order">
        {{ mod.order ?? "-" }}
      </span>
      <button
        class="btn px-1.5 py-0 leading-5"
        :disabled="last || !sortable"
        :aria-label="`Move ${mod.name} down`"
        @click="emit('down')"
      >
        ▼
      </button>
    </div>

    <ModIcon :mod="mod" />

    <div class="min-w-0 flex-1">
      <p class="flex flex-wrap items-baseline gap-x-2">
        <span class="font-semibold">{{ mod.name }}</span>
        <span class="text-sm text-muted">{{ mod.version }}</span>
        <span v-if="mod.authors.length" class="text-sm text-muted">
          by {{ mod.authors.join(", ") }}
        </span>
        <span v-if="mod.errors.length" class="text-sm text-danger">
          Not loaded
        </span>
      </p>
      <p v-if="mod.description" class="text-sm text-muted">
        {{ mod.description }}
      </p>

      <p
        v-if="tags.length || mod.unsupported.length"
        class="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs"
      >
        <span
          v-for="tag in tags"
          :key="tag.label"
          class="rounded border border-line px-1.5 py-0.5"
          :title="tag.detail"
        >
          {{ tag.label }}
        </span>
        <span
          v-for="section in mod.unsupported"
          :key="section"
          class="rounded border border-line px-1.5 py-0.5 text-muted line-through"
          title="This loader cannot apply this part of the mod yet"
        >
          {{ section }}
        </span>
      </p>

      <p v-if="mod.requires.length" class="mt-1.5 text-xs text-muted">
        needs {{ mod.requires.join(", ") }}
      </p>
      <ModNotices :mod="mod" class="mt-2" />
    </div>

    <div class="flex flex-col items-end justify-between gap-2">
      <SwitchRoot
        class="relative h-5 w-9 cursor-pointer rounded-full border border-line bg-canvas outline-accent focus-visible:outline-2 focus-visible:outline-offset-2 data-[state=checked]:border-accent data-[state=checked]:bg-accent"
        :model-value="mod.enabled"
        :aria-label="`Enable ${mod.name}`"
        @update:model-value="emit('toggle')"
      >
        <SwitchThumb
          class="block size-3.5 translate-x-0.5 rounded-full bg-muted transition-transform data-[state=checked]:translate-x-[18px] data-[state=checked]:bg-on-accent"
        />
      </SwitchRoot>
      <button
        class="btn px-2 py-0.5 text-xs"
        title="Take it out of this profile. It stays in your mods."
        @click="emit('remove')"
      >
        Remove
      </button>
    </div>
  </li>
</template>
