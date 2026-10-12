<script setup lang="ts">
import { onMounted, ref } from "vue"
import { invoke } from "@tauri-apps/api/core"
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from "reka-ui"

interface AppInfo {
  name: string
  version: string
  os: string
  arch: string
}

const tabs = [
  { value: "play", label: "Play" },
  { value: "mods", label: "Mods" },
]

const info = ref<AppInfo>()

onMounted(async () => {
  info.value = await invoke<AppInfo>("app_info")
})
</script>

<template>
  <TabsRoot class="flex h-full flex-col" default-value="play">
    <header
      class="flex items-center gap-6 border-b border-line bg-surface px-5 py-3"
    >
      <TabsList class="flex gap-1" aria-label="Sections">
        <TabsTrigger
          v-for="tab in tabs"
          :key="tab.value"
          :value="tab.value"
          class="cursor-pointer rounded-md px-3.5 py-1.5 text-muted outline-accent hover:text-ink focus-visible:outline-2 focus-visible:outline-offset-2 data-[state=active]:bg-canvas data-[state=active]:text-ink"
        >
          {{ tab.label }}
        </TabsTrigger>
      </TabsList>
    </header>

    <TabsContent class="flex-1 overflow-auto px-5 py-6" value="play">
      <p class="mb-4 text-muted">Game not located yet.</p>
      <button
        class="rounded-md bg-accent px-5 py-2 font-semibold text-on-accent outline-accent focus-visible:outline-2 focus-visible:outline-offset-2 enabled:cursor-pointer disabled:opacity-50"
        disabled
      >
        Launch
      </button>
    </TabsContent>

    <TabsContent class="flex-1 overflow-auto px-5 py-6" value="mods">
      <p class="text-muted">No mods installed.</p>
    </TabsContent>

    <footer class="min-h-8 border-t border-line px-5 py-1.5 text-sm text-muted">
      <span v-if="info">
        {{ info.name }} {{ info.version }} on {{ info.os }} {{ info.arch }}
      </span>
    </footer>
  </TabsRoot>
</template>
