import { computed, ref, type Ref } from "vue"
import type { ModView } from "./api"

// one mod and its place in the list it came from
export interface Row {
  mod: ModView
  index: number
}

// a way to order a list of mods
export interface Sort {
  label: string
  // the lower key comes first
  key: (row: Row) => number | string
}

export const byName: Sort = { label: "Name", key: ({ mod }) => mod.name }

// what does not load, then what loads with a warning, then the rest
export const byProblems: Sort = {
  label: "Problems",
  key: ({ mod }) => (mod.errors.length ? 0 : mod.warnings.length ? 1 : 2),
}

const text = new Intl.Collator(undefined, {
  sensitivity: "base",
  numeric: true,
})

// whether a mod matches what was typed into a search box
export function matches(mod: ModView, query: string) {
  const wanted = query.trim().toLowerCase()
  const fields = [mod.name, mod.id, mod.description, ...mod.authors]
  return !wanted || fields.some((field) => field.toLowerCase().includes(wanted))
}

// rows that compare equal keep the order they came in, reversed or not
export function sorted(rows: Row[], { key }: Sort, reversed: boolean) {
  return [...rows].sort((a, b) => {
    const [x, y] = [key(a), key(b)]
    const way =
      typeof x === "number" ? x - Number(y) : text.compare(x, String(y))
    return (reversed ? -way : way) || a.index - b.index
  })
}

// what a list of mods shows: the ones the search matches, in the chosen order
export function useListing<K extends string>(
  mods: () => ModView[],
  sorts: Record<K, Sort>,
  first: K,
) {
  const query = ref("")
  const sort = ref(first) as Ref<K>
  const reversed = ref(false)
  const shown = computed(() =>
    sorted(
      mods()
        .map((mod, index) => ({ mod, index }))
        .filter(({ mod }) => matches(mod, query.value)),
      sorts[sort.value],
      reversed.value,
    ),
  )
  return { query, sort, reversed, shown }
}
