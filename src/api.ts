import { invoke } from "@tauri-apps/api/core"

export interface AppInfo {
  name: string
  version: string
  os: string
  arch: string
}

export interface GameStatus {
  root: string
  // the version number the game shows, null before game 0.1.0
  version: string | null
  release: string
  backends: string[]
}

// one mod's place in a profile
export interface Entry {
  id: string
  enabled: boolean
}

export interface Content {
  label: string
  count: number
}

// a short title and the facts behind it, as [label, value] pairs
export interface Notice {
  title: string
  details: [string, string][]
}

export interface ModView {
  id: string
  name: string
  version: string
  description: string
  authors: string[]
  // the mod's own picture as a data URL
  icon: string | null
  enabled: boolean
  // place in the load order from 1, null when it does not load
  order: number | null
  // why the mod does not load
  errors: Notice[]
  warnings: Notice[]
  contents: Content[]
  unsupported: string[]
  requires: string[]
}

export interface ProfileState {
  active: string
  names: string[]
  // the mods in the active profile, in its order
  mods: ModView[]
  modsDir: string
}

export interface Inspected {
  id: string
  name: string
  version: string
  installed: string | null
  // importing would replace a newer version with an older one
  downgrade: boolean
}

export interface Imported {
  id: string
  name: string
  version: string
  previous: string | null
}

// what the game was last given
export interface InGame {
  // the profile that was applied
  profile: string
  // the mods of it that loaded, each as "id version", in load order
  mods: string[]
}

export interface LoaderStatus {
  // what the game folder holds in the loader's place, null when that cannot be told
  state: "absent" | "installed" | "outdated" | "foreign" | null
  // what the game starts with, while the loader is in it and that is on record
  inGame: InGame | null
  // the Wollay Launcher was found, so it can be opened from here
  launcher: boolean
  // why a profile cannot be applied, when it cannot
  blocked: string | null
}

// a kind of content that did not all fit the game client this PC runs
export interface Short {
  feature: string
  planned: number
  wanted: number
}

// something one of the applied mods has to say
export interface ModNote {
  // the mod's name
  name: string
  title: string
  // the facts behind the title, on one line
  detail: string
}

export interface Applied {
  // the profile whose mods the game now starts with
  profile: string
  version: string | null
  release: string
  notes: ModNote[]
  // what a mod asked for that could not be done on the game client this PC runs
  warnings: string[]
  short: Short[]
  // how many more warnings only concern the game client this PC does not run
  elsewhere: number
  // the game runs under Wine or Proton here
  wine: boolean
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  gameStatus: () => invoke<GameStatus>("game_status"),
  profiles: () => invoke<ProfileState>("profiles"),
  library: () => invoke<ModView[]>("library"),
  previewMods: (entries: Entry[]) =>
    invoke<ModView[]>("preview_mods", { entries }),
  saveProfile: (name: string, entries: Entry[]) =>
    invoke<ProfileState>("save_profile", { name, entries }),
  selectProfile: (name: string) =>
    invoke<ProfileState>("select_profile", { name }),
  createProfile: (name: string, base: string | null) =>
    invoke<ProfileState>("create_profile", { name, base }),
  renameProfile: (name: string, to: string) =>
    invoke<ProfileState>("rename_profile", { name, to }),
  deleteProfile: (name: string) =>
    invoke<ProfileState>("delete_profile", { name }),
  inspectMod: (path: string) => invoke<Inspected>("inspect_mod", { path }),
  importMod: (path: string) => invoke<Imported>("import_mod", { path }),
  deleteMod: (id: string) => invoke<void>("delete_mod", { id }),
  loaderStatus: () => invoke<LoaderStatus>("loader_status"),
  applyProfile: () => invoke<Applied>("apply_profile"),
  openWollayLauncher: () => invoke<void>("open_wollay_launcher"),
  removeLoader: () => invoke<boolean>("remove_loader"),
}
