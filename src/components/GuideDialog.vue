<script setup lang="ts">
import { computed, nextTick, ref } from "vue"
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from "reka-ui"
import { api } from "../api"
import { pages, render, target } from "../docs"

const open = defineModel<boolean>("open", { required: true })

const current = ref(pages[0]?.file ?? "")
const page = computed(() => pages.find((each) => each.file === current.value))
const html = computed(() => (page.value ? render(page.value) : ""))
const content = ref<HTMLElement>()
const problem = ref("")

async function show(file: string, anchor = "") {
  problem.value = ""
  current.value = file
  await nextTick()
  const place = anchor
    ? content.value?.querySelector(`#${CSS.escape(anchor)}`)
    : null
  if (place) place.scrollIntoView({ block: "start" })
  else content.value?.scrollTo({ top: 0 })
}

// links stay in the guide, except web pages, which open in the browser
function follow(event: MouseEvent) {
  const link = (event.target as Element).closest("a")
  if (!link || !page.value) return
  event.preventDefault()
  const to = target(link.getAttribute("href") ?? "", page.value.file)
  if (!to) return
  if ("web" in to) {
    api.openLink(to.web).catch((reason) => (problem.value = String(reason)))
  } else {
    show(to.page, to.anchor)
  }
}
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="overlay" />
      <DialogContent
        class="dialog flex h-[85vh] w-240 max-w-[95vw] flex-col p-0"
        @open-auto-focus.prevent
      >
        <div class="flex items-center gap-3 border-b border-line px-5 py-3">
          <DialogTitle class="flex-1 font-semibold">Modding guide</DialogTitle>
          <DialogDescription class="sr-only">
            How to make mods for Cube World Omega
          </DialogDescription>
          <DialogClose class="btn">Close</DialogClose>
        </div>
        <p
          v-if="problem"
          class="border-b border-line px-5 py-2 text-sm text-danger select-text"
        >
          {{ problem }}
        </p>
        <div class="flex min-h-0 flex-1">
          <nav
            class="flex w-52 shrink-0 flex-col gap-0.5 overflow-auto border-r border-line p-2"
          >
            <button
              v-for="each in pages"
              :key="each.file"
              class="cursor-pointer rounded px-2.5 py-1.5 text-left text-sm hover:bg-canvas"
              :class="{ 'bg-canvas font-semibold': each.file === current }"
              @click="show(each.file)"
            >
              {{ each.title }}
            </button>
          </nav>
          <!-- the pages are this repo's own files, and markdown-it leaves raw HTML out -->
          <article
            ref="content"
            class="doc min-w-0 flex-1 overflow-auto px-6 py-4 select-text"
            @click="follow"
            v-html="html"
          />
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
