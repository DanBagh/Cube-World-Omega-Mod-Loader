# Cube World Omega Mod Launcher (CWOML)

A launcher and mod manager for Cube World Omega.

> [!WARNING]
> Currently a scaffold. It opens a window and nothing game related works yet.

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

The code is in `src-tauri/src/`. Tauri settings are in
`src-tauri/tauri.conf.json`.

Functions marked `#[tauri::command]` in `lib.rs` are what the frontend can
call with `invoke`.

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

- Windows: `src-tauri/target/release/cwo-mod-launcher.exe`
- Linux: an AppImage under `src-tauri/target/release/bundle/`
- macOS: a `.app` under `src-tauri/target/release/bundle/macos/`

Run that file to start the built app. It can be copied anywhere first.

## Data

The launcher keeps its data in a `cwoml-data` folder beside the executable
(beside the AppImage file on Linux). If that folder cannot be created, and
always on macOS, it uses the per-user app data folder instead.

## License

GPL v3. See [LICENSE](LICENSE).

The app icon is made from a Cube World Omega model. That artwork belongs to
the game's author and is not covered by this license.
