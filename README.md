# Cube World Omega Mod Loader (CWOML)

A mod manager for Cube World Omega.

> [!INFO]
> This loader only applies a collection of mods to the game. You still start the game from the Wollay Launcher itself.

Goals:

- OS agnostic (CWO currently builds for Windows but I believe can run on Linux with some effort)
- preferably portable: no installer, and nothing for players to have to install. download and run. need to figure out the best placement for the data folder.
- supporting plain dll injection but implement a preferable data-driven approach for better mod compatibility and management with custom schema (in Rust).

Tech Stack: Tauri 2, Vue 3, TypeScript, Vite, Tailwind CSS and Reka UI.

## Contributor/Developer Setup

Install:

1. [Bun](https://bun.sh) for the frontend tooling
2. [Rust](https://rustup.rs) for the backend
3. The system packages Tauri needs for your OS, listed at
   https://tauri.app/start/prerequisites/
   - Windows: [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
     with the "Desktop development with C++" workload
   - macOS: `xcode-select --install`
   - Linux: the packages for your distro on that page

Clone repo and open in IDE of your choice, then run in terminal:

```
bun install
```

## Starting the app

```
bun run tauri dev
```

This starts the frontend dev server, builds the backend and opens the window.
The first build takes a few minutes. After that, frontend changes show up
right away and backend changes rebuild on save. Close the window or press
Ctrl+C in the terminal to stop it.

## Frontend

The code is in `src/`. It is Vue with Tailwind classes and Reka UI components.

To work on the UI in a browser without building the backend, only Bun is
needed:

```
bun run dev
```

Open http://localhost:1420. Calls to the backend do not work there.

## Backend

The backend is a Cargo workspace:

- `crates/core/` is the mod loader as a library, with no Tauri in it: finding
  the game, reading and ordering mods, analysing the client DLLs and planning
  the patches. Each kind of mod content is one module in `src/features/`.
- `crates/loader/` is the DLL that runs inside the game. It applies the plan
  the library writes and does nothing else. The library's build script builds
  it and embeds it, so the app stays one file.
- `crates/cli/` is a small command line tool over the library, for trying
  things without the app:
  `cargo run -p cwoml-cli -- plan --mods <folder> --out <folder>`
- `src-tauri/` is the app. Tauri settings are in `src-tauri/tauri.conf.json`.

Functions marked `#[tauri::command]` in `src-tauri/src/lib.rs` are what the
frontend can call with `invoke`.

The tests run without the game:
`cargo test -p cwoml-core -p cwoml-loader`

Some features put a little machine code of their own into the game. Its source
is in `crates/core/src/features/asm/`, and the assembled bytes are in the Rust
file of the feature. After changing an `.asm`, this assembles them all and
compares, printing the new bytes of any that differ. It needs `ml64` from the
MSVC build tools:
`cargo test -p cwoml-core -- --ignored trampolines`

## Before committing

```
bun run check
```

This formats the code and checks that both sides compile. Without Rust
installed, `bun run check:frontend` does the frontend half.

VS Code formats files on save once the recommended extensions are installed.

## Building a release

```
bun run tauri build
```

There are no installers. The output is:

- Windows: `target/release/cwo-mod-loader.exe`
- Linux: an AppImage under `target/release/bundle/`
- macOS: a `.app` under `target/release/bundle/macos/`

Run that file to start the built app. It can be copied anywhere first.

## Releasing

Releases are built by GitHub Actions when a version tag is pushed. The
version is kept in one place: `version` under `[workspace.package]` in
`Cargo.toml`.

1. Set the new version there.
2. Add a section for it to `CHANGELOG.md`, following
   [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
3. Commit, then tag and push the tag:
   `git tag v0.2.0` and `git push origin v0.2.0`

The workflow checks that the tag matches the version, builds the Windows exe
and publishes a GitHub release with that changelog section as its notes. Each
folder in `example_mods/` is zipped and attached to the release too.

## Data

The loader keeps its data in a `cwoml-data` folder beside the executable
(beside the AppImage file on Linux). If that folder cannot be created, and
always on macOS, it uses the per-user app data folder instead.

Mods are imported in the app as `.zip` files, from Manage mods or by dropping
them on the window. A mod zip has a `mod.json` at its top level, or
inside a single top-level folder. It is unpacked into `cwoml-data/mods/<id>`.
Importing a mod that is already there replaces it, and asks first when the
zip is the older version.

Every imported mod is in that one folder, whichever profiles use it. Deleting
a mod removes it from the folder and from every profile.

A mod can ship a picture for the mod list: an `icon.png` beside `mod.json`,
or the file named by `"icon"` in `mod.json` (png, jpg or webp, up to 512 KB).

Profiles are in `cwoml-data/profiles.json`. A profile lists the mods added to
it, which of them are on, and their order. A new profile starts empty or as a
copy of another one.

## Applying a profile

Apply makes the game start with the mods of the current profile. It works out
their patches, writes them to `cwoml-data/plan.txt` and puts the in-game DLL
into the game folder. It does not start the game. Game must be started from the Wollay
Launcher itself.

Two files are put into the game folder, beside `cubeworld.exe`:
`dbghelp.dll`, which is the in-game DLL, and `cwoml.txt`, which tells it where
`cwoml-data` is. None of the game's own files are changed. Disable mods takes
both out again, and the game starts plain. Enable mods puts them back with
the current profile. A `dbghelp.dll` that is not ours is never replaced
or removed.

The in-game DLL goes by that name because `cubeworld.exe` asks for `dbghelp.dll`
itself, so Windows always finds the one beside it. It hands every call on to
the real one in the Windows folder.

What was applied is noted in `profiles.json`. The mod list is grayed out
while the mods are off or the game has another profile, and a line above it
says when the profile has changed since it was applied.

A game update installs into a new folder without the in-game DLL, so the mods
show as off. Enable them again after one.

When the game starts, the in-game DLL writes what it did to
`cwoml-data/loader.log`. A patch whose place in the game no longer looks the
way the plan expects is skipped and named there.

## License

GPL v3. See [LICENSE](LICENSE).

The app icon is made from a Cube World Omega model. That artwork belongs to
the game's author and is not covered by this license.
