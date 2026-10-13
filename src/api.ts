import { invoke } from "@tauri-apps/api/core"

export interface AppInfo {
  name: string
  version: string
  os: string
  arch: string
}

export interface GameStatus {
  root: string
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
}
