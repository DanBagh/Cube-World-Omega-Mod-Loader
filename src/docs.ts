import MarkdownIt from "markdown-it"

// the guide is docs/ in the repo, bundled when the app is built
const sources = import.meta.glob<string>("../docs/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
})
// pictures the pages show, by their path from docs/
const pictures = import.meta.glob<string>(
  "../docs/**/*.{png,jpg,jpeg,gif,webp,svg}",
  { query: "?url", import: "default", eager: true },
)

// where the repo's other files are read from
const REPO =
  "https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/blob/main/docs/"
// the first page. Its links to the other pages give their order
const HOME = "README.md"

export interface Page {
  // its file in docs/, the name links give it
  file: string
  title: string
  source: string
}

const all: Page[] = Object.entries(sources).map(([path, source]) => {
  const file = path.slice("../docs/".length)
  const title = /^# (.+)$/m.exec(source)?.[1].trim() ?? file
  return { file, title, source }
})

function linked(source: string): string[] {
  const files = [...source.matchAll(/\]\((?:\.\/)?([\w.-]+\.md)(?:#[^)]*)?\)/g)]
  return files.map((link) => link[1])
}

const home = all.find((page) => page.file === HOME)
const order = [HOME, ...linked(home?.source ?? "")]
const rank = (page: Page) => {
  const at = order.indexOf(page.file)
  return at < 0 ? order.length : at
}
export const pages = all.sort(
  (a, b) => rank(a) - rank(b) || a.file.localeCompare(b.file),
)

// raw HTML in a page shows as text
const markdown = new MarkdownIt({ html: false })

// what one rendering of a page keeps track of: the heading ids given so far
type Rendering = { slugs: Map<string, number> }

// the id GitHub gives a heading, so a link to it works there and here
function slug(text: string, seen: Map<string, number>) {
  const base = text
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-")
  const count = seen.get(base) ?? 0
  seen.set(base, count + 1)
  return count ? `${base}-${count}` : base
}

markdown.renderer.rules.heading_open = (tokens, at, options, env, self) => {
  const text = (tokens[at + 1].children ?? [])
    .filter((part) => part.type === "text" || part.type === "code_inline")
    .map((part) => part.content)
    .join("")
  const { slugs } = env as Rendering
  tokens[at].attrSet("id", slug(text, slugs))
  return self.renderToken(tokens, at, options)
}

const image = markdown.renderer.rules.image!
markdown.renderer.rules.image = (tokens, at, options, env, self) => {
  const src = String(tokens[at].attrGet("src") ?? "").replace(/^\.\//, "")
  const bundled = pictures[`../docs/${src}`]
  if (bundled) tokens[at].attrSet("src", bundled)
  return image(tokens, at, options, env, self)
}

export function render(page: Page): string {
  const rendering: Rendering = { slugs: new Map() }
  return markdown.render(page.source, rendering)
}

// where a link goes: a page of the guide and a place on it, or a web page
export type Target = { page: string; anchor: string } | { web: string }

export function target(href: string, from: string): Target | null {
  if (href.startsWith("https://")) return { web: href }
  // mailto:, http: and the like are not followed
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return null
  const [path, anchor = ""] = href.split("#")
  const file = path.replace(/^\.\//, "")
  if (!file) return { page: from, anchor: decodeURIComponent(anchor) }
  if (pages.some((page) => page.file === file)) {
    return { page: file, anchor: decodeURIComponent(anchor) }
  }
  // anything else in the repo is read on GitHub
  return { web: new URL(href, REPO).href }
}
