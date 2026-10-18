//! The script runtime: each mod's Luau scripts, run on the client's main thread.
//!
//! Every mod gets a Luau state of its own, sandboxed: its globals and the libraries are
//! read-only, there is no file, process or network access, and it cannot see another
//! mod's state. Scripts reach the game only through the `cwo` table. A script that
//! errors, runs too long or takes too much memory is stopped, and only that mod's
//! scripts are turned off.
//!
//! The runtime starts on the first frame, from the frame hook, so everything a script
//! does happens on the thread the client draws its frames on. `docs/scripting.md` is the
//! guide to `cwo` for modders.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use mlua::{
    Function, Lua, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic, VmState,
};

use crate::log::log;
use crate::plan::Plan;
use crate::ui::{self, Anchor, Engine, Event, Game, Menu, Popup, Side};

/// How long a mod's scripts may take to start, and to handle one event.
const START_BUDGET: Duration = Duration::from_millis(500);
const EVENT_BUDGET: Duration = Duration::from_millis(20);
/// The most memory a mod's scripts may hold.
const MEMORY: usize = 64 << 20;
/// The most of an error message that goes to the log.
const MAX_ERROR: usize = 600;
/// The events a menu and a popup have.
const MENU_EVENTS: [&str; 4] = ["open", "close", "frame", "draw"];

/// What the plan asks the runtime to run.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    pub language: String,
    /// Each mod's scripts in load order: the mod's id, a name for the script and its
    /// code.
    pub scripts: Vec<(String, String, String)>,
    /// Each mod's strings by key.
    pub texts: HashMap<String, HashMap<String, String>>,
}

static SOURCES: OnceLock<Sources> = OnceLock::new();

/// Reads the scripts the plan names, and notes the menus it declares, for the first
/// frame.
pub fn prepare(plan: &Plan) {
    let mut sources = Sources {
        language: plan.language.clone().unwrap_or_else(|| "en".into()),
        ..Sources::default()
    };
    for (owner, file) in &plan.scripts {
        match std::fs::read_to_string(file) {
            Ok(code) => {
                let name = std::path::Path::new(file)
                    .file_name()
                    .map_or_else(|| file.clone(), |n| n.to_string_lossy().into_owned());
                sources.scripts.push((owner.clone(), name, code));
            }
            Err(e) => log!("{owner}: could not read {file}: {e}"),
        }
    }
    for (owner, key, text) in &plan.texts {
        sources
            .texts
            .entry(owner.clone())
            .or_default()
            .insert(key.clone(), text.clone());
    }
    if !sources.scripts.is_empty() {
        log!("scripts: {} to run", sources.scripts.len());
    }
    crate::controls::declare(&plan.menus);
    ui::declare(plan.menus.clone());
    let _ = SOURCES.set(sources);
}

/// `text` with `{1}`, `{2}` ... replaced by the arguments, in turn.
pub fn fill(text: &str, arguments: &[String]) -> String {
    let mut out = text.to_owned();
    for (index, argument) in arguments.iter().enumerate() {
        out = out.replace(&format!("{{{}}}", index + 1), argument);
    }
    out
}

/// A mod's handlers, kept with its Luau state: its own frames, and its menus' and
/// popups' events by menu id and popup id.
#[derive(Default)]
struct Handlers {
    frame: Vec<Function>,
    menus: HashMap<String, HashMap<&'static str, Function>>,
    popups: HashMap<(String, String), Function>,
}

fn runtime_error(text: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(text.into())
}

/// A color from a script: `{r, g, b}`, 0 to 255 each.
fn color(value: &Value) -> mlua::Result<[f32; 3]> {
    let table = value
        .as_table()
        .ok_or_else(|| runtime_error("a color is {r, g, b}"))?;
    let mut rgb = [0f32; 3];
    for (index, channel) in rgb.iter_mut().enumerate() {
        let value: f64 = table.get(index + 1)?;
        *channel = value.clamp(0.0, 255.0) as f32;
    }
    Ok(rgb)
}

fn color_table(lua: &Lua, rgb: [f32; 3]) -> mlua::Result<Table> {
    lua.create_sequence_from(rgb.map(f64::from))
}

/// A field of an options table, if it is there.
fn option<T: mlua::FromLua>(options: &Option<Table>, key: &str) -> mlua::Result<Option<T>> {
    match options {
        Some(table) => table.get(key),
        None => Ok(None),
    }
}

/// The engine, for an API call. Scripts only run once it is made.
fn engine() -> mlua::Result<Rc<Engine>> {
    ui::engine().ok_or_else(|| runtime_error("the game is not ready"))
}

fn font(game: &Game, name: Option<String>) -> mlua::Result<Option<ui::Font>> {
    let title = match name.as_deref() {
        None | Some("text") => false,
        Some("title") => true,
        Some(other) => return Err(runtime_error(format!("no font called '{other}'"))),
    };
    Ok(game.font(title))
}

fn size(width: i32, height: i32) -> mlua::Result<(i32, i32)> {
    if ui::fits((width, height)) {
        Ok((width, height))
    } else {
        Err(runtime_error(format!(
            "a menu is at least 8 by 8 and at most {} voxels across all its {} layers",
            ui::MAX_VOXELS,
            ui::DEPTH
        )))
    }
}

/// A script's handle on its menu.
struct MenuHandle(Rc<Menu>);

impl UserData for MenuHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.0.id.clone()));
        fields.add_field_method_get("open", |_, this| Ok(this.0.is_open()));
        fields.add_field_method_get("width", |_, this| Ok(this.0.wanted.get().0));
        fields.add_field_method_get("height", |_, this| Ok(this.0.wanted.get().1));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("on", |lua, this, (event, handler): (String, Function)| {
            let event = MENU_EVENTS
                .into_iter()
                .find(|e| *e == event)
                .ok_or_else(|| runtime_error(format!("a menu has no event called '{event}'")))?;
            let mut handlers = lua
                .app_data_mut::<Handlers>()
                .ok_or_else(|| runtime_error("no handlers"))?;
            handlers
                .menus
                .entry(this.0.id.clone())
                .or_default()
                .insert(event, handler);
            Ok(())
        });
        methods.add_method("show", |_, this, ()| {
            this.0.request.set(Some(true));
            Ok(())
        });
        methods.add_method("hide", |_, this, ()| {
            this.0.request.set(Some(false));
            Ok(())
        });
        methods.add_method("redraw", |_, this, ()| {
            this.0.dirty.set(true);
            Ok(())
        });
        methods.add_method("size", |_, this, (width, height): (i32, i32)| {
            let wanted = size(width, height)?;
            if this.0.wanted.replace(wanted) != wanted {
                this.0.dirty.set(true);
            }
            Ok(())
        });
        methods.add_method("cursor", |_, this, ()| {
            Ok(match this.0.cursor.get() {
                Some((x, y)) => (Some(x), Some(y)),
                None => (None, None),
            })
        });
        methods.add_method("popup", |_, this, id: String| {
            Ok(PopupHandle {
                popup: this.0.popup(&id),
                menu: this.0.clone(),
            })
        });
    }
}

/// A script's handle on a popup of its menu.
struct PopupHandle {
    menu: Rc<Menu>,
    popup: Rc<Popup>,
}

impl UserData for PopupHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.popup.id.clone()));
        fields.add_field_method_get("shown", |_, this| Ok(this.popup.shown.get().is_some()));
        fields.add_field_method_get("width", |_, this| Ok(this.popup.wanted.get().0));
        fields.add_field_method_get("height", |_, this| Ok(this.popup.wanted.get().1));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("on", |lua, this, (event, handler): (String, Function)| {
            if event != "draw" {
                return Err(runtime_error(format!(
                    "a popup has no event called '{event}'"
                )));
            }
            let mut handlers = lua
                .app_data_mut::<Handlers>()
                .ok_or_else(|| runtime_error("no handlers"))?;
            handlers
                .popups
                .insert((this.menu.id.clone(), this.popup.id.clone()), handler);
            Ok(())
        });
        methods.add_method("show", |_, this, options: Option<Table>| {
            let side: Option<String> = option(&options, "side")?;
            let side = match side {
                None => Side::Right,
                Some(name) => Side::named(&name)
                    .ok_or_else(|| runtime_error(format!("no side called '{name}'")))?,
            };
            this.popup.shown.set(Some(Anchor {
                side,
                gap: option(&options, "gap")?.unwrap_or(10),
                along: option(&options, "along")?.unwrap_or(0),
            }));
            Ok(())
        });
        methods.add_method("hide", |_, this, ()| {
            this.popup.shown.set(None);
            Ok(())
        });
        methods.add_method("redraw", |_, this, ()| {
            this.popup.dirty.set(true);
            Ok(())
        });
        methods.add_method("size", |_, this, (width, height): (i32, i32)| {
            let wanted = size(width, height)?;
            if this.popup.wanted.replace(wanted) != wanted {
                this.popup.dirty.set(true);
            }
            Ok(())
        });
    }
}

/// What a script draws on, while its draw handler runs and no longer.
struct CanvasHandle {
    game: Rc<Game>,
    menu: Rc<Menu>,
    popup: Option<Rc<Popup>>,
    valid: Rc<Cell<bool>>,
}

impl CanvasHandle {
    fn canvas(&self) -> mlua::Result<&ui::Canvas> {
        if !self.valid.get() {
            return Err(runtime_error(
                "a canvas is drawn on in its draw handler only",
            ));
        }
        Ok(match &self.popup {
            Some(popup) => popup.canvas(),
            None => self.menu.canvas(),
        })
    }
}

impl UserData for CanvasHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("width", |_, this| Ok(this.canvas()?.size().0));
        fields.add_field_method_get("height", |_, this| Ok(this.canvas()?.size().1));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("panel", |_, this, color_value: Option<Value>| {
            let color = match color_value {
                Some(value) => color(&value)?,
                None => ui::PANEL,
            };
            // SAFETY: in the draw handler, on the main thread, the canvas shaped
            unsafe { this.game.panel(this.canvas()?, &color) };
            Ok(())
        });
        methods.add_method(
            "text",
            |_, this, (x, y, text, options): (i32, i32, String, Option<Table>)| {
                let font = font(&this.game, option(&options, "font")?)?;
                let Some(font) = font else {
                    return Ok(0);
                };
                let color = match option::<Value>(&options, "color")? {
                    Some(value) => color(&value)?,
                    None => ui::TEXT,
                };
                let z = option(&options, "z")?.unwrap_or(ui::TEXT_Z);
                let depth = option(&options, "depth")?.unwrap_or(ui::TEXT_DEPTH);
                // SAFETY: in the draw handler, on the main thread, the canvas shaped
                Ok(unsafe {
                    this.game.write(
                        this.canvas()?,
                        font,
                        text.as_bytes(),
                        (x, y, z, depth),
                        &color,
                    )
                })
            },
        );
        methods.add_method(
            "fill",
            |_, this, (x, y, width, height, value, options): (i32, i32, i32, i32, Value, Option<Table>)| {
                let rgb = if value.is_nil() {
                    None
                } else {
                    Some(color(&value)?.map(|c| c as u8))
                };
                let z = option(&options, "z")?.unwrap_or(ui::TEXT_Z);
                let depth = option(&options, "depth")?.unwrap_or(1);
                // SAFETY: in the draw handler, on the main thread, the canvas shaped
                unsafe {
                    this.game
                        .fill(this.canvas()?, (x, y, z), (width, height, depth), rgb)
                };
                Ok(())
            },
        );
        methods.add_method(
            "voxel",
            |_, this, (x, y, value, z): (i32, i32, Value, Option<i32>)| {
                let rgb = if value.is_nil() {
                    None
                } else {
                    Some(color(&value)?.map(|c| c as u8))
                };
                // SAFETY: in the draw handler, on the main thread, the canvas shaped
                unsafe {
                    this.game.fill(
                        this.canvas()?,
                        (x, y, z.unwrap_or(ui::TEXT_Z)),
                        (1, 1, 1),
                        rgb,
                    )
                };
                Ok(())
            },
        );
    }
}

/// An item, as its bytes were when it was asked for.
struct Item {
    game: Rc<Game>,
    bytes: Vec<u8>,
}

impl UserData for Item {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("type", |_, this| {
            Ok(u32::from_le_bytes(
                this.bytes[..4].try_into().unwrap_or_default(),
            ))
        });
        fields.add_field_method_get("name", |_, this| Ok(this.game.name(&this.bytes)));
        fields.add_field_method_get("rarity", |_, this| Ok(this.game.rarity(&this.bytes).0));
        fields.add_field_method_get("color", |lua, this| {
            this.game
                .rarity(&this.bytes)
                .1
                .map(|rgb| color_table(lua, rgb))
                .transpose()
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("stat", |_, this, name: String| {
            let number = ui::STATS
                .iter()
                .find(|(stat, _)| *stat == name)
                .map(|(_, number)| *number)
                .ok_or_else(|| runtime_error(format!("no stat called '{name}'")))?;
            Ok(f64::from(this.game.stat(&this.bytes, number)))
        });
    }
}

/// One mod's scripts.
struct Mod {
    id: String,
    lua: Lua,
    /// When what runs now has to be done by.
    deadline: Rc<Cell<Option<Instant>>>,
    failed: bool,
}

impl Mod {
    /// A sandboxed state with the `cwo` table, its scripts run.
    fn start(id: &str, sources: &Sources) -> Mod {
        let mut started = Mod {
            id: id.to_owned(),
            lua: Lua::new(),
            deadline: Rc::new(Cell::new(None)),
            failed: false,
        };
        if let Err(e) = started.set_up(sources) {
            started.fail(&e);
            return started;
        }
        for (owner, name, code) in &sources.scripts {
            if owner != id || started.failed {
                continue;
            }
            let chunk = started
                .lua
                .load(code.as_str())
                .set_name(format!("={id}/{name}"));
            if let Err(e) = started.within(START_BUDGET, || chunk.exec()) {
                started.fail(&e);
            }
        }
        started
    }

    fn set_up(&self, sources: &Sources) -> mlua::Result<()> {
        let lua = &self.lua;
        lua.set_memory_limit(MEMORY)?;
        let deadline = self.deadline.clone();
        lua.set_interrupt(move |_| match deadline.get() {
            Some(by) if Instant::now() > by => Err(runtime_error("took too long")),
            _ => Ok(VmState::Continue),
        });
        lua.set_app_data(Handlers::default());

        let cwo = lua.create_table()?;
        let id = self.id.clone();
        cwo.set(
            "log",
            lua.create_function(move |_, parts: Variadic<Value>| {
                let parts: Vec<String> = parts
                    .iter()
                    .map(|part| part.to_string().unwrap_or_else(|_| "?".into()))
                    .collect();
                log!("[{id}] {}", parts.join(" "));
                Ok(())
            })?,
        )?;
        let texts = sources.texts.get(&self.id).cloned().unwrap_or_default();
        cwo.set(
            "text",
            lua.create_function(move |_, (key, arguments): (String, Variadic<Value>)| {
                let arguments: Vec<String> = arguments
                    .iter()
                    .map(|a| a.to_string().unwrap_or_else(|_| "?".into()))
                    .collect();
                Ok(fill(texts.get(&key).unwrap_or(&key), &arguments))
            })?,
        )?;
        cwo.set("language", sources.language.as_str())?;
        cwo.set("version", env!("CARGO_PKG_VERSION"))?;
        cwo.set(
            "on",
            lua.create_function(|lua, (event, handler): (String, Function)| {
                let mut handlers = lua
                    .app_data_mut::<Handlers>()
                    .ok_or_else(|| runtime_error("no handlers"))?;
                match event.as_str() {
                    "frame" => handlers.frame.push(handler),
                    other => return Err(runtime_error(format!("no event called '{other}'"))),
                }
                Ok(())
            })?,
        )?;
        let owner = self.id.clone();
        cwo.set(
            "menu",
            lua.create_function(move |_, id: String| {
                let menu = engine()?.menu(&owner, &id).ok_or_else(|| {
                    runtime_error(format!("no menu '{id}': declare it in mod.json"))
                })?;
                Ok(MenuHandle(menu))
            })?,
        )?;
        cwo.set(
            "number",
            lua.create_function(|_, value: f64| Ok(ui::number(value)))?,
        )?;

        let player = lua.create_table()?;
        player.set(
            "gear",
            lua.create_function(|_, ()| {
                let engine = engine()?;
                Ok(engine
                    .game
                    .gear()
                    .into_iter()
                    .map(|bytes| Item {
                        game: engine.game.clone(),
                        bytes,
                    })
                    .collect::<Vec<_>>())
            })?,
        )?;
        cwo.set("player", player)?;

        let interface = lua.create_table()?;
        interface.set(
            "width",
            lua.create_function(|_, (text, name): (String, Option<String>)| {
                let engine = engine()?;
                Ok(font(&engine.game, name)?.map_or(0, |f| f.width(text.as_bytes())))
            })?,
        )?;
        interface.set(
            "height",
            lua.create_function(|_, name: Option<String>| {
                let engine = engine()?;
                Ok(font(&engine.game, name)?.map_or(0, |f| f.1))
            })?,
        )?;
        interface.set(
            "fit",
            lua.create_function(|_, (text, room, name): (String, i32, Option<String>)| {
                let engine = engine()?;
                let Some(font) = font(&engine.game, name)? else {
                    return Ok(text);
                };
                let fitted = ui::fit(text.as_bytes(), room, |s| font.width(s));
                Ok(String::from_utf8_lossy(&fitted).into_owned())
            })?,
        )?;
        let colors = lua.create_table()?;
        for (name, rgb) in [
            ("panel", ui::PANEL),
            ("title", ui::TITLE),
            ("text", ui::TEXT),
            ("muted", ui::MUTED),
        ] {
            colors.set(name, color_table(lua, rgb)?)?;
        }
        interface.set("colors", colors)?;
        interface.set("depth", ui::DEPTH)?;
        cwo.set("ui", interface)?;

        lua.globals().set("cwo", cwo)?;
        // globals and libraries read-only from here, `cwo` with them
        lua.sandbox(true)
    }

    /// Runs `what` with `budget` to do it in.
    fn within<T>(
        &self,
        budget: Duration,
        what: impl FnOnce() -> mlua::Result<T>,
    ) -> mlua::Result<T> {
        self.deadline.set(Some(Instant::now() + budget));
        let result = what();
        self.deadline.set(None);
        result
    }

    fn fail(&mut self, error: &mlua::Error) {
        let mut text = error.to_string();
        if text.len() > MAX_ERROR {
            text.truncate(text.floor_char_boundary(MAX_ERROR));
            text.push_str(" ...");
        }
        log!("{}: script stopped: {text}", self.id);
        self.failed = true;
    }

    /// Runs a handler with the event budget.
    fn call(&mut self, handler: &Function, argument: impl mlua::IntoLuaMulti) {
        if self.failed {
            return;
        }
        if let Err(e) = self.within(EVENT_BUDGET, || handler.call::<()>(argument)) {
            self.fail(&e);
        }
    }

    fn frame(&mut self, seconds: f64) {
        if self.failed {
            return;
        }
        let handlers = match self.lua.app_data_ref::<Handlers>() {
            Some(handlers) => handlers.frame.clone(),
            None => return,
        };
        for handler in handlers {
            self.call(&handler, seconds);
        }
    }

    fn menu_event(&mut self, menu: &Menu, event: Event) {
        if self.failed {
            return;
        }
        let handler = {
            let Some(handlers) = self.lua.app_data_ref::<Handlers>() else {
                return;
            };
            match &event {
                Event::Draw(_, Some(popup)) => handlers
                    .popups
                    .get(&(menu.id.clone(), popup.id.clone()))
                    .cloned(),
                _ => {
                    let name = match event {
                        Event::Open => "open",
                        Event::Close => "close",
                        Event::Frame(_) => "frame",
                        Event::Draw(..) => "draw",
                    };
                    handlers
                        .menus
                        .get(&menu.id)
                        .and_then(|h| h.get(name))
                        .cloned()
                }
            }
        };
        let Some(handler) = handler else {
            return;
        };
        match event {
            Event::Open | Event::Close => self.call(&handler, ()),
            Event::Frame(seconds) => self.call(&handler, seconds),
            Event::Draw(menu, popup) => {
                let Some(engine) = ui::engine() else {
                    return;
                };
                let valid = Rc::new(Cell::new(true));
                let canvas = CanvasHandle {
                    game: engine.game.clone(),
                    menu: menu.clone(),
                    popup: popup.cloned(),
                    valid: valid.clone(),
                };
                match self.lua.create_userdata(canvas) {
                    Ok(canvas) => self.call(&handler, canvas),
                    Err(e) => self.fail(&e),
                }
                valid.set(false);
            }
        }
    }
}

/// Every mod's scripts.
pub struct Runtime {
    mods: Vec<Mod>,
}

impl Runtime {
    pub fn start(sources: &Sources) -> Runtime {
        let mut ids: Vec<&str> = Vec::new();
        for (owner, ..) in &sources.scripts {
            if !ids.contains(&owner.as_str()) {
                ids.push(owner);
            }
        }
        let mods: Vec<Mod> = ids.iter().map(|id| Mod::start(id, sources)).collect();
        let running = mods.iter().filter(|m| !m.failed).count();
        if !mods.is_empty() {
            log!("scripts: {running} of {} mod(s) running", mods.len());
        }
        Runtime { mods }
    }
}

thread_local! {
    static RUNTIME: RefCell<Option<Runtime>> = const { RefCell::new(None) };
}

/// Starts the scripts. The engine calls it once it is made, on the first frame.
pub fn start() {
    let Some(sources) = SOURCES.get().filter(|s| !s.scripts.is_empty()) else {
        return;
    };
    let runtime = Runtime::start(sources);
    RUNTIME.with(|slot| *slot.borrow_mut() = Some(runtime));
}

/// The scripts' own frame handlers.
pub fn frame(seconds: f64) {
    RUNTIME.with(|runtime| {
        if let Ok(mut runtime) = runtime.try_borrow_mut()
            && let Some(runtime) = runtime.as_mut()
        {
            for running in &mut runtime.mods {
                running.frame(seconds);
            }
        }
    });
}

/// Has the script of the mod that owns `menu` handle an event of it.
pub fn menu_event(menu: &Menu, event: Event) {
    RUNTIME.with(|runtime| {
        if let Ok(mut runtime) = runtime.try_borrow_mut()
            && let Some(runtime) = runtime.as_mut()
            && let Some(owner) = runtime.mods.iter_mut().find(|m| m.id == menu.owner)
        {
            owner.menu_event(menu, event);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(scripts: &[(&str, &str)], texts: &[(&str, &str, &str)]) -> Sources {
        let mut sources = Sources {
            language: "en".into(),
            ..Sources::default()
        };
        for (owner, code) in scripts {
            sources
                .scripts
                .push((owner.to_string(), "main.luau".into(), code.to_string()));
        }
        for (owner, key, text) in texts {
            sources
                .texts
                .entry(owner.to_string())
                .or_default()
                .insert(key.to_string(), text.to_string());
        }
        sources
    }

    #[test]
    fn fills_in_arguments() {
        assert_eq!(fill("{1} OF {2}", &["3".into(), "7".into()]), "3 OF 7");
        assert_eq!(fill("NO ARGUMENTS", &[]), "NO ARGUMENTS");
    }

    #[test]
    fn a_script_gets_its_own_strings_and_runs_each_frame() {
        let code = r#"
            assert(cwo.text("title") == "GEAR STATS")
            assert(cwo.text("count", 3) == "3 ITEMS")
            assert(cwo.text("missing") == "missing")
            assert(cwo.language == "en")
            assert(cwo.number(12.6) == "13")
            frames = 0
            cwo.on("frame", function(seconds)
                assert(type(seconds) == "number")
                frames += 1
                if frames == 2 then error("second frame") end
            end)
        "#;
        let texts = [("a", "title", "GEAR STATS"), ("a", "count", "{1} ITEMS")];
        let mut runtime = Runtime::start(&sources(&[("a", code)], &texts));
        assert!(!runtime.mods[0].failed);
        runtime.mods[0].frame(0.016);
        assert!(!runtime.mods[0].failed);
        // the handler errors on the second frame, which stops that mod
        runtime.mods[0].frame(0.016);
        assert!(runtime.mods[0].failed);
    }

    #[test]
    fn mods_are_kept_apart_and_sandboxed() {
        let first = r#"
            shared = "first"
            assert(os == nil or os.execute == nil, "no processes")
            assert(io == nil, "no files")
            assert(not pcall(function() cwo.text = nil end), "cwo is read-only")
        "#;
        let second = r#"
            assert(shared == nil, "another mod's globals")
            assert(cwo.text("title") == "title", "another mod's strings")
        "#;
        let runtime = Runtime::start(&sources(
            &[("a", first), ("b", second)],
            &[("a", "title", "A")],
        ));
        assert_eq!(
            runtime.mods.iter().map(|m| m.failed).collect::<Vec<_>>(),
            [false, false]
        );
    }

    #[test]
    fn a_script_that_does_not_stop_is_stopped() {
        let runtime = Runtime::start(&sources(&[("a", "while true do end"), ("b", "")], &[]));
        assert_eq!(
            runtime.mods.iter().map(|m| m.failed).collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn the_example_scripts_compile() {
        let lua = Lua::new();
        for (name, code) in [(
            "gear-stats",
            include_str!("../../../example_mods/gear-stats/scripts/gear.luau"),
        )] {
            if let Err(e) = lua.load(code).set_name(name).into_function() {
                panic!("{name}: {e}");
            }
        }
    }

    #[test]
    fn unknown_events_and_menus_are_errors() {
        let runtime = Runtime::start(&sources(
            &[
                ("a", r#"cwo.on("never", print)"#),
                ("b", r#"cwo.menu("x")"#),
            ],
            &[],
        ));
        assert_eq!(
            runtime.mods.iter().map(|m| m.failed).collect::<Vec<_>>(),
            [true, true]
        );
    }
}
