use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cwoml_core::features;
use cwoml_core::game::{self, DEFAULT_CHANNEL, Game};
use cwoml_core::loader;
use cwoml_core::mods::{self, Mod, Notice};
use cwoml_core::profiles::{self, Entry, InGame, PROFILES_NAME, Profiles};
use cwoml_core::report::Level;
use serde::Serialize;
use tauri::{Manager, State, WebviewWindowBuilder};

const DATA_DIR: &str = "cwoml-data";
const MODS_DIR: &str = "mods";
/// A mod icon larger than this is not shown.
const MAX_ICON: u64 = 512 * 1024;
/// Where new versions of this app are published.
const RELEASES: &str = "https://github.com/DanBagh/Cube-World-Omega-Mod-Loader/releases/latest";

struct Paths {
    data: PathBuf,
}

impl Paths {
    fn mods(&self) -> PathBuf {
        self.data.join(MODS_DIR)
    }

    fn profiles(&self) -> PathBuf {
        self.data.join(PROFILES_NAME)
    }
}

#[derive(Serialize)]
struct AppInfo {
    name: String,
    version: String,
    os: &'static str,
    arch: &'static str,
}

#[derive(Serialize)]
struct GameStatus {
    root: String,
    /// The version number the game shows, None before game 0.1.0.
    version: Option<String>,
    release: String,
    backends: Vec<&'static str>,
}

#[derive(Serialize)]
struct Content {
    label: &'static str,
    count: usize,
}

/// One mod as the lists show it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModView {
    id: String,
    name: String,
    version: String,
    description: String,
    authors: Vec<String>,
    /// The mod's own picture as a data URL.
    icon: Option<String>,
    enabled: bool,
    /// Place in the load order, from 1. None for a mod that does not load.
    order: Option<usize>,
    /// Why the mod does not load.
    errors: Vec<Notice>,
    warnings: Vec<Notice>,
    contents: Vec<Content>,
    /// Manifest sections the loader cannot apply yet.
    unsupported: Vec<&'static str>,
    requires: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileState {
    active: String,
    names: Vec<String>,
    /// The mods in the active profile, in its order.
    mods: Vec<ModView>,
    mods_dir: String,
}

/// What a zip holds, before it is imported.
#[derive(Serialize)]
struct Inspected {
    id: String,
    name: String,
    version: String,
    /// Version installed now, if the mod is.
    installed: Option<String>,
    /// Importing would replace a newer version with an older one.
    downgrade: bool,
}

#[derive(Serialize)]
struct Imported {
    id: String,
    name: String,
    version: String,
    /// Version this import replaced, if it replaced one.
    previous: Option<String>,
}

/// Whether a profile can be applied to the game, and what the game folder holds in the
/// loader's place.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoaderStatus {
    /// "absent", "installed", "outdated" or "foreign". None when that cannot be told.
    state: Option<&'static str>,
    /// What the game starts with, while the loader is in it and that is on record.
    in_game: Option<InGame>,
    /// The Wollay Launcher was found, so it can be opened from here.
    launcher: bool,
    /// Why a profile cannot be applied, when it cannot.
    blocked: Option<String>,
}

/// A kind of content that did not all fit the client this PC runs.
#[derive(Serialize)]
struct Short {
    feature: &'static str,
    planned: usize,
    wanted: usize,
}

/// Something one of the applied mods has to say.
#[derive(Serialize)]
struct ModNote {
    /// The mod's name.
    name: String,
    title: String,
    /// The facts behind the title, on one line.
    detail: String,
}

#[derive(Serialize)]
struct Applied {
    /// The profile whose mods the game now starts with.
    profile: String,
    version: Option<String>,
    release: String,
    notes: Vec<ModNote>,
    /// What a mod asked for that could not be done on the client this PC runs.
    warnings: Vec<String>,
    short: Vec<Short>,
    /// How many more warnings only concern the client this PC does not run.
    elsewhere: usize,
    /// The game runs under Wine or Proton here.
    wine: bool,
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    let package = app.package_info();
    AppInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// The install the official launcher manages, or why it was not found.
#[tauri::command]
fn game_status() -> Result<GameStatus, String> {
    let game = game::find(None, DEFAULT_CHANNEL).map_err(|e| e.to_string())?;
    Ok(GameStatus {
        root: game.root.display().to_string(),
        version: game.version.clone(),
        release: game.release,
        backends: game.modules.keys().map(|backend| backend.key()).collect(),
    })
}

/// The installed game, which mods are judged against.
fn installed() -> Option<Game> {
    game::find(None, DEFAULT_CHANNEL).ok()
}

/// What the mods are judged against without a game. A mod made for another release still
/// loads unless it says strict, so "unknown" is safe.
fn judged(game: Option<&Game>) -> mods::Installed<'_> {
    game.map_or("unknown".into(), Into::into)
}

fn icon(found: &Mod) -> Option<String> {
    let path = found.icon()?;
    if std::fs::metadata(&path).ok()?.len() > MAX_ICON {
        return None;
    }
    let kind = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpeg",
        "webp" => "webp",
        _ => "png",
    };
    let data = std::fs::read(&path).ok()?;
    Some(format!(
        "data:image/{kind};base64,{}",
        STANDARD.encode(data)
    ))
}

fn view(found: &Mod, order: Option<usize>) -> ModView {
    let contents = features::contents(found);
    ModView {
        id: found.id.clone(),
        name: found.name.clone(),
        version: found.version.to_string(),
        description: found.description().to_owned(),
        authors: found.authors().into_iter().map(str::to_owned).collect(),
        icon: icon(found),
        enabled: found.enabled,
        order,
        errors: found.errors.clone(),
        warnings: found.warnings.clone(),
        contents: contents
            .supported
            .into_iter()
            .map(|(label, count)| Content { label, count })
            .collect(),
        unsupported: contents.unsupported,
        requires: found
            .object("dependencies")
            .map(|deps| deps.keys().cloned().collect())
            .unwrap_or_default(),
    }
}

/// The mods `entries` name, in that order, with what loads.
fn evaluate(mods_dir: &Path, entries: &[Entry]) -> Vec<ModView> {
    let mut found = mods::load(mods_dir);
    profiles::apply(&mut found, entries);
    let game = installed();
    let order = mods::resolve(&mut found, judged(game.as_ref()));
    let mut listed: Vec<usize> = (0..found.len())
        .filter(|&index| entries.iter().any(|entry| entry.id == found[index].id))
        .collect();
    listed.sort_by_key(|&index| found[index].rank);
    listed
        .into_iter()
        .map(|index| {
            let place = order.iter().position(|&i| i == index).map(|at| at + 1);
            view(&found[index], place)
        })
        .collect()
}

fn read_profiles(paths: &Paths) -> Result<Profiles, String> {
    let file = paths.profiles();
    let fresh = !file.is_file();
    let profiles = Profiles::load(&file, || {
        let game = installed();
        profiles::entries_for(&mut mods::load(&paths.mods()), judged(game.as_ref()))
    });
    // written at once: from here on a profile only holds the mods added to it
    if fresh {
        profiles.save(&file).map_err(|e| e.to_string())?;
    }
    Ok(profiles)
}

fn state(paths: &Paths, profiles: &Profiles) -> ProfileState {
    let mods_dir = paths.mods();
    ProfileState {
        active: profiles.active.clone(),
        names: profiles.names(),
        mods: evaluate(&mods_dir, &profiles.active().mods),
        mods_dir: mods_dir.display().to_string(),
    }
}

/// Reads the profiles, changes them, saves them and returns the new state.
fn change(
    paths: &Paths,
    edit: impl FnOnce(&mut Profiles) -> cwoml_core::Result<()>,
) -> Result<ProfileState, String> {
    let mut profiles = read_profiles(paths)?;
    edit(&mut profiles).map_err(|e| e.to_string())?;
    profiles
        .save(&paths.profiles())
        .map_err(|e| e.to_string())?;
    Ok(state(paths, &profiles))
}

#[tauri::command]
fn profiles(paths: State<Paths>) -> Result<ProfileState, String> {
    Ok(state(&paths, &read_profiles(&paths)?))
}

/// Every mod in the collection, whichever profiles use it.
#[tauri::command]
fn library(paths: State<Paths>) -> Vec<ModView> {
    let mut found = mods::load(&paths.mods());
    found.sort_by_key(|item| item.name.to_lowercase());
    found.iter().map(|item| view(item, None)).collect()
}

/// What a profile would look like with `entries`, without saving anything.
#[tauri::command]
fn preview_mods(paths: State<Paths>, entries: Vec<Entry>) -> Vec<ModView> {
    evaluate(&paths.mods(), &entries)
}

#[tauri::command]
fn save_profile(
    paths: State<Paths>,
    name: String,
    entries: Vec<Entry>,
) -> Result<ProfileState, String> {
    change(&paths, |profiles| profiles.set(&name, entries))
}

#[tauri::command]
fn select_profile(paths: State<Paths>, name: String) -> Result<ProfileState, String> {
    change(&paths, |profiles| profiles.select(&name))
}

/// Adds a profile, empty or starting as a copy of the profile called `base`.
#[tauri::command]
fn create_profile(
    paths: State<Paths>,
    name: String,
    base: Option<String>,
) -> Result<ProfileState, String> {
    change(&paths, |profiles| {
        let entries = match &base {
            Some(base) => profiles
                .get(base)
                .map(|profile| profile.mods.clone())
                .ok_or_else(|| {
                    cwoml_core::Error::invalid(format!("there is no profile called {base}"))
                })?,
            None => Vec::new(),
        };
        profiles.create(&name, entries)
    })
}

#[tauri::command]
fn rename_profile(paths: State<Paths>, name: String, to: String) -> Result<ProfileState, String> {
    change(&paths, |profiles| profiles.rename(&name, &to))
}

#[tauri::command]
fn delete_profile(paths: State<Paths>, name: String) -> Result<ProfileState, String> {
    change(&paths, |profiles| profiles.delete(&name))
}

#[tauri::command]
fn inspect_mod(paths: State<Paths>, path: String) -> Result<Inspected, String> {
    let zip = mods::inspect_zip(Path::new(&path), &paths.mods()).map_err(|e| e.to_string())?;
    Ok(Inspected {
        id: zip.id,
        name: zip.name,
        version: zip.version.to_string(),
        installed: zip.installed.map(|version| version.to_string()),
        downgrade: zip
            .installed
            .is_some_and(|installed| zip.version < installed),
    })
}

/// Unpacks a mod zip into the collection, replacing the installed version if there is one.
#[tauri::command]
fn import_mod(paths: State<Paths>, path: String) -> Result<Imported, String> {
    let imported = mods::import_zip(Path::new(&path), &paths.mods()).map_err(|e| e.to_string())?;
    Ok(Imported {
        id: imported.id,
        name: imported.name,
        version: imported.version.to_string(),
        previous: imported.previous.map(|version| version.to_string()),
    })
}

/// Deletes a mod from the collection and takes it out of every profile.
#[tauri::command]
fn delete_mod(paths: State<Paths>, id: String) -> Result<(), String> {
    mods::remove(&paths.mods(), &id).map_err(|e| e.to_string())?;
    change(&paths, |profiles| {
        profiles.forget(&id);
        Ok(())
    })
    .map(|_| ())
}

/// Notes what the game was given. None once the mods are taken out of it.
fn record(paths: &Paths, in_game: Option<InGame>) -> Result<(), String> {
    let mut profiles = read_profiles(paths)?;
    profiles.in_game = in_game;
    profiles.save(&paths.profiles()).map_err(|e| e.to_string())
}

#[tauri::command]
fn loader_status(paths: State<Paths>) -> LoaderStatus {
    let launcher = loader::official_launcher().is_some();
    let blocked = |why: String| LoaderStatus {
        state: None,
        in_game: None,
        launcher,
        blocked: Some(why),
    };
    let Some(dll) = loader::embedded() else {
        return blocked(
            "This build of the app does not have the part that runs inside the game.".into(),
        );
    };
    let game = match game::find(None, DEFAULT_CHANNEL) {
        Ok(game) => game,
        Err(reason) => return blocked(reason.to_string()),
    };
    let (state, blocked) = match loader::state(&game.root, dll) {
        loader::State::Absent => ("absent", None),
        loader::State::Installed { current: true } => ("installed", None),
        loader::State::Installed { current: false } => ("outdated", None),
        loader::State::Foreign => (
            "foreign",
            Some(format!(
                "The game folder has a {} that this app did not put there.",
                loader::DLL_NAME
            )),
        ),
    };
    // the folder decides: a game update leaves the loader behind, whatever is on record
    let in_game = matches!(state, "installed" | "outdated")
        .then(|| read_profiles(&paths).ok()?.in_game)
        .flatten();
    LoaderStatus {
        state: Some(state),
        in_game,
        launcher,
        blocked,
    }
}

/// Plans the active profile's mods and puts the loader into the game folder, so the game
/// starts with them. Starting the game stays with the Wollay Launcher.
// not on the main thread: analyzing the client takes a few seconds
#[tauri::command(async)]
fn apply_profile(paths: State<Paths>) -> Result<Applied, String> {
    let game = game::find(None, DEFAULT_CHANNEL).map_err(|e| e.to_string())?;
    let profiles = read_profiles(&paths)?;
    let mut found = mods::load(&paths.mods());
    profiles::apply(&mut found, &profiles.active().mods);
    let name = &profiles.active().name;
    let build =
        loader::apply(&game, &mut found, &paths.data, Some(name)).map_err(|e| e.to_string())?;
    let loaded = build.order.iter().map(|&index| &found[index]);
    record(&paths, Some(InGame::new(name, loaded)))?;
    let client = game.client();
    let (here, elsewhere): (Vec<_>, Vec<_>) = build
        .report
        .lines
        .iter()
        .filter(|line| line.level == Level::Warning)
        .partition(|line| line.client.is_none() || line.client == client);
    let notes = build
        .order
        .iter()
        .flat_map(|&index| {
            let applied = &found[index];
            applied.warnings.iter().map(|notice| ModNote {
                name: applied.name.clone(),
                title: notice.title.clone(),
                detail: notice.to_string(),
            })
        })
        .collect();
    Ok(Applied {
        profile: name.clone(),
        version: game.version.clone(),
        release: game.release,
        notes,
        // every such line starts by naming its client, which says nothing to a player
        warnings: here
            .iter()
            .map(|line| {
                let named = line.client.map(|client| format!("{}: ", client.key()));
                let text = named.and_then(|prefix| line.text.strip_prefix(&prefix));
                text.unwrap_or(&line.text).to_owned()
            })
            .collect(),
        short: build
            .tallies
            .iter()
            .filter(|tally| Some(tally.backend) == client && tally.planned < tally.wanted)
            .map(|tally| Short {
                feature: tally.feature,
                planned: tally.planned,
                wanted: tally.wanted,
            })
            .collect(),
        elsewhere: elsewhere.len(),
        wine: cfg!(not(windows)),
    })
}

/// Opens the Wollay Launcher, the only program that can start the game signed in.
#[tauri::command]
fn open_wollay_launcher() -> Result<(), String> {
    let exe =
        loader::official_launcher().ok_or("The Wollay Launcher was not found on this computer.")?;
    loader::open_apart(&exe).map_err(|e| e.to_string())
}

/// Opens the page of the newest release in the browser.
#[tauri::command]
fn open_releases() -> Result<(), String> {
    browse(RELEASES)
}

/// Opens a link from the guide in the browser. Only web pages, over https.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("{url} is not a web page this app opens"));
    }
    browse(&url)
}

fn browse(url: &str) -> Result<(), String> {
    let opener = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(opener)
        .arg(url)
        .spawn()
        .map_err(|e| format!("{opener}: {e}"))?;
    // waited for, so it does not linger once it has handed the page to the browser
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Takes the loader out of the game folder. False if it was not there.
#[tauri::command]
fn remove_loader(paths: State<Paths>) -> Result<bool, String> {
    let game = game::find(None, DEFAULT_CHANNEL).map_err(|e| e.to_string())?;
    let removed = loader::clear(&game, &paths.data).map_err(|e| e.to_string())?;
    record(&paths, None)?;
    Ok(removed)
}

// portable: data sits beside the executable, falling back to the per-user dir
fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    // a .app bundle must not be written into
    if !cfg!(target_os = "macos") {
        // an AppImage runs from a read-only mount, APPIMAGE is the file itself
        let exe = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok());
        let dir = exe
            .as_deref()
            .and_then(|path| path.parent())
            .map(|parent| parent.join(DATA_DIR));
        if let Some(dir) = dir
            && std::fs::create_dir_all(&dir).is_ok()
        {
            return dir;
        }
    }
    app.path().app_data_dir().expect("no app data directory")
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = data_dir(app.handle());
            // ignore a failure here: the mods list is then simply empty
            let _ = std::fs::create_dir_all(data.join(MODS_DIR));
            let webview = data.join("webview");
            // before the window exists, so its first command finds the state
            app.manage(Paths { data });
            // the window is built here so the webview keeps its files in the data dir
            let config = &app.config().app.windows[0];
            WebviewWindowBuilder::from_config(app.handle(), config)?
                .data_directory(webview)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            game_status,
            profiles,
            library,
            preview_mods,
            save_profile,
            select_profile,
            create_profile,
            rename_profile,
            delete_profile,
            inspect_mod,
            import_mod,
            delete_mod,
            loader_status,
            apply_profile,
            open_wollay_launcher,
            open_releases,
            open_link,
            remove_loader
        ])
        .run(tauri::generate_context!())
        .expect("failed to run app");
}
