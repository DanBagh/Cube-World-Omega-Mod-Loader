<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { open as pickFiles } from "@tauri-apps/plugin-dialog"
import {
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuPortal,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuRoot,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "reka-ui"
import {
  api,
  type Applied,
  type AppInfo,
  type Entry,
  type GameStatus,
  type ModView,
  type LoaderStatus,
  type ProfileState,
} from "./api"
import AppliedDialog from "./components/AppliedDialog.vue"
import ConfirmDialog, { type Question } from "./components/ConfirmDialog.vue"
import LibraryDialog from "./components/LibraryDialog.vue"
import ModCard from "./components/ModCard.vue"
import NewProfileDialog from "./components/NewProfileDialog.vue"
import NoticeBar from "./components/NoticeBar.vue"
import RenameProfileDialog from "./components/RenameProfileDialog.vue"
import SortPicker from "./components/SortPicker.vue"
import { byName, byProblems, useListing, type Sort } from "./listing"

const info = ref<AppInfo>()
const game = ref<GameStatus>()
const gameError = ref("")

const active = ref("")
const names = ref<string[]>([])
// the profile as shown, which may be ahead of what it has saved
const mods = ref<ModView[]>([])
const saved = ref<Entry[]>([])
// every imported mod, whichever profiles use it
const library = ref<ModView[]>([])
const message = ref("")
const note = ref("")
const dropping = ref(false)

// what the game folder holds in the loader's place, and whether a profile can be applied
const loader = ref<LoaderStatus>()
const applying = ref(false)
const applied = ref<Applied>()

const browsing = ref(false)
const creating = ref(false)
const renaming = ref(false)
const question = ref<Question>()
let settle: (yes: boolean) => void = () => {}

const entries = computed<Entry[]>(() =>
  mods.value.map(({ id, enabled }) => ({ id, enabled })),
)
const dirty = computed(
  () => JSON.stringify(entries.value) !== JSON.stringify(saved.value),
)
const enabled = computed(() => mods.value.filter((mod) => mod.enabled).length)

// ways to order the list. A mod's place in the profile is its load order
const sorts = {
  order: { label: "Load order", key: ({ index }) => index },
  name: byName,
  enabled: { label: "Enabled", key: ({ mod }) => (mod.enabled ? 0 : 1) },
  problems: byProblems,
} satisfies Record<string, Sort>
const { query, sort, reversed, shown } = useListing(
  () => mods.value,
  sorts,
  "order",
)
// moving a mod only means something with the whole list in load order
const movable = computed(
  () =>
    shown.value.length === mods.value.length &&
    sort.value === "order" &&
    !reversed.value,
)

function show(state: ProfileState) {
  active.value = state.active
  names.value = state.names
  mods.value = state.mods
  saved.value = entries.value
  query.value = ""
}

// runs a backend call and puts a failure where the user can see it
async function attempt(action: () => Promise<void>) {
  message.value = ""
  note.value = ""
  try {
    await action()
  } catch (reason) {
    message.value = String(reason)
  }
}

function ask(asked: Question) {
  question.value = asked
  return new Promise<boolean>((resolve) => (settle = resolve))
}

function answer(yes: boolean) {
  question.value = undefined
  settle(yes)
}

async function stage(next: Entry[]) {
  mods.value = await api.previewMods(next)
}

async function refreshLibrary() {
  library.value = await api.library()
}

const without = (id: string) => entries.value.filter((entry) => entry.id !== id)

const toggle = (id: string) =>
  attempt(() =>
    stage(
      entries.value.map((entry) =>
        entry.id === id ? { ...entry, enabled: !entry.enabled } : entry,
      ),
    ),
  )

function move(index: number, by: number) {
  const next = [...entries.value]
  const [entry] = next.splice(index, 1)
  next.splice(index + by, 0, entry)
  return attempt(() => stage(next))
}

const addToProfile = (id: string) =>
  attempt(() => stage([...entries.value, { id, enabled: true }]))

// the mod stays in the collection
const removeFromProfile = (id: string) => attempt(() => stage(without(id)))

const save = () =>
  attempt(async () => show(await api.saveProfile(active.value, entries.value)))

const discard = () => attempt(async () => show(await api.profiles()))

async function select(name: string) {
  if (name === active.value) return
  await attempt(async () => show(await api.selectProfile(name)))
}

const create = (name: string, base: string | null) =>
  attempt(async () => show(await api.createProfile(name, base)))

const rename = (to: string) =>
  attempt(async () => {
    show(await api.renameProfile(active.value, to))
    // what the game has is on record under the profile's name
    await refreshLoader()
  })

async function deleteProfile() {
  const sure = await ask({
    title: "Delete profile",
    text: `Delete the profile ${active.value}? Its mods stay in your mods.`,
    confirm: "Delete",
  })
  if (sure) {
    await attempt(async () => show(await api.deleteProfile(active.value)))
  }
}

async function deleteMod(mod: ModView) {
  const sure = await ask({
    title: "Delete mod",
    text: `Delete ${mod.name} from this computer? It is taken out of every profile.`,
    confirm: "Delete",
  })
  if (!sure) return
  await attempt(async () => {
    await api.deleteMod(mod.id)
    // not a change to this profile: the mod is gone from all of them
    saved.value = saved.value.filter((entry) => entry.id !== mod.id)
    await refreshLibrary()
    await stage(without(mod.id))
    note.value = `Deleted ${mod.name}.`
  })
}

// unpacks one zip into the collection. Empty when the user backs out
async function importZip(path: string) {
  const zip = await api.inspectMod(path)
  if (zip.downgrade) {
    const sure = await ask({
      title: "Replace with an older version",
      text: `${zip.name} ${zip.installed} is installed. Replace it with the older ${zip.version}?`,
      confirm: "Replace",
    })
    if (!sure) return
  }
  return api.importMod(path)
}

async function importZips(paths: string[]) {
  const zips = paths.filter((path) => path.toLowerCase().endsWith(".zip"))
  if (!zips.length) {
    message.value = "Mods are added as .zip files."
    return
  }
  await attempt(async () => {
    const next = [...entries.value]
    const done: string[] = []
    const failed: string[] = []
    for (const path of zips) {
      try {
        const mod = await importZip(path)
        if (!mod) continue
        done.push(
          mod.previous
            ? `Replaced ${mod.name} ${mod.previous} with ${mod.version}.`
            : `Added ${mod.name} ${mod.version}.`,
        )
        if (!next.some((entry) => entry.id === mod.id)) {
          next.push({ id: mod.id, enabled: true })
        }
      } catch (reason) {
        failed.push(String(reason))
      }
    }
    await refreshLibrary()
    await stage(next)
    note.value = done.join(" ")
    message.value = failed.join(" ")
  })
}

async function pickZips() {
  const picked = await pickFiles({
    multiple: true,
    filters: [{ name: "Mod", extensions: ["zip"] }],
  })
  if (picked) await importZips([picked].flat())
}

// why the profile cannot be applied right now, empty when it can
const blocked = computed(() => {
  if (applying.value) return "Applying the profile"
  if (dirty.value) return "Save or discard your changes first"
  if (!loader.value) return "Looking for the game"
  return loader.value.blocked ?? ""
})
const installed = computed(() =>
  ["installed", "outdated"].includes(loader.value?.state ?? ""),
)
// the mods of the shown profile that load, named the way an apply puts them on record
const loading = computed(() =>
  mods.value
    .filter((mod) => mod.order)
    .sort((a, b) => (a.order ?? 0) - (b.order ?? 0))
    .map((mod) => `${mod.id} ${mod.version}`),
)
// how the shown profile stands to what the game starts with
const standing = computed(() => {
  const state = loader.value?.state
  if (state === "absent") return "off"
  if (!installed.value) return undefined
  const given = loader.value?.inGame
  if (JSON.stringify(given?.mods) === JSON.stringify(loading.value)) {
    return state === "installed" ? "applied" : "changed"
  }
  return given?.profile === active.value ? "changed" : "other"
})
// the profile the game has, while it is still one of them
const given = computed(() => {
  const name = loader.value?.inGame?.profile
  return name !== undefined && names.value.includes(name) ? name : undefined
})
// nothing shown here is what the game starts with
const idle = computed(() => ["off", "other"].includes(standing.value ?? ""))
// what stands between the shown profile and the game, empty when nothing does
const status = computed(() => {
  if (loader.value?.blocked) return loader.value.blocked
  switch (standing.value) {
    case "off":
      return "Mods are off. The game starts without them."
    case "other":
      return given.value
        ? `Not applied. The game still has ${given.value}.`
        : "Not applied. The game has other mods."
    case "changed":
      // unsaved changes come first, and Save is already offered for them
      return dirty.value ? "" : "Press Apply to update the mods."
    default:
      return ""
  }
})

async function refreshLoader() {
  loader.value = await api.loaderStatus()
}

// makes the game start with the saved profile's mods. It does not start the game: only
// the Wollay Launcher can do that signed in
async function apply() {
  applying.value = true
  await attempt(async () => {
    applied.value = await api.applyProfile()
  })
  applying.value = false
  await refreshLoader().catch(() => {})
}

function openWollayLauncher() {
  applied.value = undefined
  return attempt(() => api.openWollayLauncher())
}

// one press back to the plain game. Nothing is asked: Enable mods puts them back
const disableMods = () =>
  attempt(async () => {
    await api.removeLoader()
    await refreshLoader()
  })

// the folder may have changed outside this app
watch(browsing, (open) => {
  if (open) attempt(refreshLibrary)
})

let stopListening = () => {}

onMounted(async () => {
  await attempt(async () => {
    info.value = await api.appInfo()
    show(await api.profiles())
    await refreshLibrary()
  })
  try {
    game.value = await api.gameStatus()
  } catch (reason) {
    gameError.value = String(reason)
  }
  await refreshLoader().catch(() => {})
  stopListening = await getCurrentWebview().onDragDropEvent(({ payload }) => {
    dropping.value = payload.type === "enter" || payload.type === "over"
    if (payload.type === "drop") importZips(payload.paths)
  })
})

onUnmounted(() => stopListening())
</script>

<template>
  <div class="flex h-full flex-col">
    <header
      class="flex items-center gap-2 border-b border-line bg-surface px-4 py-2.5"
    >
      <DropdownMenuRoot>
        <DropdownMenuTrigger
          class="btn flex min-w-40 items-center justify-between gap-3"
          aria-label="Profile"
          :disabled="dirty"
          :title="dirty ? 'Save or discard your changes first' : undefined"
        >
          {{ active }}
          <span class="text-xs text-muted">▼</span>
        </DropdownMenuTrigger>
        <DropdownMenuPortal>
          <DropdownMenuContent
            class="z-10 min-w-(--reka-dropdown-menu-trigger-width) rounded-md border border-line bg-surface p-1 shadow-lg"
            align="start"
            :side-offset="4"
          >
            <DropdownMenuRadioGroup
              :model-value="active"
              @update:model-value="select(String($event))"
            >
              <DropdownMenuRadioItem
                v-for="name in names"
                :key="name"
                :value="name"
                class="menu-item data-[state=checked]:font-semibold"
              >
                {{ name }}
              </DropdownMenuRadioItem>
            </DropdownMenuRadioGroup>
            <DropdownMenuSeparator class="my-1 h-px bg-line" />
            <DropdownMenuItem class="menu-item" @select="creating = true">
              New profile...
            </DropdownMenuItem>
            <DropdownMenuItem class="menu-item" @select="renaming = true">
              Rename {{ active }}...
            </DropdownMenuItem>
            <DropdownMenuItem
              class="menu-item text-danger"
              :disabled="names.length < 2"
              @select="deleteProfile"
            >
              Delete {{ active }}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenuPortal>
      </DropdownMenuRoot>

      <button class="btn" @click="browsing = true">Manage mods</button>

      <template v-if="dirty">
        <button class="btn-primary" @click="save">Save</button>
        <button class="btn" @click="discard">Discard</button>
      </template>

      <div class="ml-auto flex items-center gap-2">
        <button
          v-if="installed"
          class="btn"
          :disabled="applying"
          title="Take the mods out of the game, so it starts plain. Your mods and profiles stay here."
          @click="disableMods"
        >
          Disable mods
        </button>
        <button
          class="px-6"
          :class="standing === 'applied' ? 'btn' : 'btn-primary'"
          :disabled="!!blocked"
          :title="
            blocked ||
            'Make the game start with this profile. You still start the game from the Wollay Launcher.'
          "
          @click="apply"
        >
          {{
            applying
              ? "Applying..."
              : standing === "off"
                ? "Enable mods"
                : "Apply"
          }}
        </button>
      </div>
    </header>

    <main
      class="flex-1 overflow-auto px-4 py-4"
      :class="{ 'outline-2 -outline-offset-4 outline-accent': dropping }"
    >
      <p
        v-if="status"
        class="mb-3 rounded-md border px-3 py-2 text-sm"
        :class="
          standing === 'off'
            ? 'border-line text-muted'
            : 'border-warn text-warn'
        "
      >
        {{ status }}
      </p>

      <div class="mb-3 flex items-center gap-2">
        <h1 class="font-semibold">Mods</h1>
        <span v-if="enabled !== mods.length" class="text-sm text-muted">
          {{ enabled }} / {{ mods.length }} enabled
        </span>
        <div v-if="mods.length" class="ml-auto flex items-center gap-2">
          <SortPicker
            v-model:sort="sort"
            v-model:reversed="reversed"
            :sorts="sorts"
          />
          <input
            v-model="query"
            class="field w-64"
            type="search"
            placeholder="Search"
            aria-label="Search"
          />
        </div>
      </div>

      <NoticeBar v-if="!browsing" class="mb-3" :error="message" :note="note" />

      <p v-if="!mods.length" class="py-10 text-center text-muted">
        {{
          library.length
            ? "This profile has no mods. Add some in Manage mods."
            : "No mods yet. Import a .zip in Manage mods, or drop one on this window."
        }}
      </p>
      <p v-else-if="!shown.length" class="py-10 text-center text-muted">
        No mod in this profile matches that search.
      </p>
      <ul
        v-else
        class="flex flex-col gap-2 transition-[opacity,filter]"
        :class="{ 'opacity-70 grayscale': idle }"
      >
        <ModCard
          v-for="{ mod, index } in shown"
          :key="mod.id"
          :mod="mod"
          :first="index === 0"
          :last="index === mods.length - 1"
          :sortable="movable"
          @toggle="toggle(mod.id)"
          @up="move(index, -1)"
          @down="move(index, 1)"
          @remove="removeFromProfile(mod.id)"
        />
      </ul>
    </main>

    <footer
      class="flex min-h-8 gap-4 border-t border-line px-4 py-1.5 text-sm text-muted"
    >
      <span v-if="info">CWOML {{ info.version }}</span>
      <span>|</span>
      <span v-if="standing">
        Currently Running:
        {{ standing === "off" ? "no mods" : (given ?? "other mods") }}
      </span>
      <span class="ml-auto truncate" :title="game?.release">
        Cube World Omega
        {{
          game
            ? (game.version ?? game.release)
            : gameError || "Looking for the game"
        }}
      </span>
    </footer>

    <LibraryDialog
      v-model:open="browsing"
      :mods="library"
      :used="mods.map((mod) => mod.id)"
      :profile="active"
      @add="addToProfile"
      @delete="deleteMod"
      @import="pickZips"
    >
      <NoticeBar class="mt-3" :error="message" :note="note" />
    </LibraryDialog>
    <NewProfileDialog
      v-model:open="creating"
      :profiles="names"
      @create="create"
    />
    <RenameProfileDialog
      v-model:open="renaming"
      :name="active"
      @rename="rename"
    />
    <AppliedDialog
      :applied="applied"
      :launcher="loader?.launcher ?? false"
      @close="applied = undefined"
      @launcher="openWollayLauncher"
    />
    <ConfirmDialog :question="question" @answer="answer" />
  </div>
</template>

<!-- DEBUG -->
<style lang="css">
/* * {
  outline: 1px solid red;
} */
</style>
