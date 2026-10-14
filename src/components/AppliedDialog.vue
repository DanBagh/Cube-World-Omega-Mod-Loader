<script setup lang="ts">
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui"
import type { Applied } from "../api"

defineProps<{
  // what was just applied. The parent clears it when the dialog closes
  applied?: Applied
  // the Wollay Launcher was found, so it can be opened from here
  launcher: boolean
}>()
const emit = defineEmits<{ close: []; launcher: [] }>()
</script>

<template>
  <DialogRoot
    :open="applied !== undefined"
    @update:open="(open) => open || emit('close')"
  >
    <DialogPortal>
      <DialogOverlay class="overlay" />
      <DialogContent
        v-if="applied"
        class="dialog flex max-h-[85vh] w-140 flex-col"
      >
        <DialogTitle class="font-semibold">
          {{ applied.profile }} applied
        </DialogTitle>
        <DialogDescription class="mt-2 text-sm text-muted">
          The game will now start with this profile's mods. You still need to
          start the game from the Wollay Launcher.
        </DialogDescription>
        <p class="mt-1 text-xs text-muted" :title="applied.release">
          Game version {{ applied.version ?? applied.release }}
        </p>
        <p v-if="applied.wine" class="mt-2 text-sm text-muted select-text">
          Under Wine or Proton the mods only load with the DLL override
          <code>dbghelp=n,b</code> set for the Wollay Launcher. See the README
          for how.
        </p>

        <ul
          v-if="
            applied.short.length ||
            applied.warnings.length ||
            applied.notes.length
          "
          class="mt-3 flex flex-col gap-1 overflow-auto text-sm text-warn select-text"
        >
          <li v-for="fit in applied.short" :key="fit.feature">
            Only {{ fit.planned }} of {{ fit.wanted }} {{ fit.feature }} fit
            this game version
          </li>
          <li v-for="warning in applied.warnings" :key="warning">
            {{ warning }}
          </li>
          <li
            v-for="note in applied.notes"
            :key="note.name + note.detail"
            :title="note.detail"
          >
            {{ note.name }}: {{ note.title }}
          </li>
        </ul>
        <p v-if="applied.elsewhere" class="mt-2 text-xs text-muted">
          {{ applied.elsewhere }} more
          {{ applied.elsewhere === 1 ? "note is" : "notes are" }} only about the
          build of the game that this PC does not use.
        </p>

        <div class="mt-5 flex justify-end gap-2">
          <DialogClose class="btn">Close</DialogClose>
          <button v-if="launcher" class="btn-primary" @click="emit('launcher')">
            Open Wollay Launcher
          </button>
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
