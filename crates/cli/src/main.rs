use std::path::PathBuf;
use std::process::ExitCode;

use cwoml_core::features::{self, Build};
use cwoml_core::game::{self, DEFAULT_CHANNEL, Game};
use cwoml_core::mods::{self, Mod};
use cwoml_core::plan::PLAN_NAME;

const USAGE: &str = "\
cwoml status --mods DIR [--game-dir DIR] [--channel NAME]
    show the game install and which mods would load, in order
cwoml plan --mods DIR --out DIR [--game-dir DIR] [--channel NAME]
    work out the patches and write plan.txt, with any converted models, to the out folder
cwoml import --mods DIR --zip FILE
    unpack a mod zip into the mods folder

Without --game-dir the install managed by the official launcher is used.
Nothing is written to the game folder.";

#[derive(Default)]
struct Args {
    command: String,
    mods: Option<PathBuf>,
    game_dir: Option<PathBuf>,
    channel: Option<String>,
    out: Option<PathBuf>,
    zip: Option<PathBuf>,
}

fn parse(mut raw: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut args = Args {
        command: raw.next().ok_or("no command")?,
        ..Args::default()
    };
    while let Some(flag) = raw.next() {
        let mut value = || raw.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--mods" => args.mods = Some(value()?.into()),
            "--game-dir" => args.game_dir = Some(value()?.into()),
            "--channel" => args.channel = Some(value()?),
            "--out" => args.out = Some(value()?.into()),
            "--zip" => args.zip = Some(value()?.into()),
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(args)
}

fn print_game(game: &Game) {
    println!("Game release {} at {}", game.release, game.root.display());
    for (backend, path) in &game.modules {
        println!("  {:<9} {}", backend.key(), path.display());
    }
}

fn print_mods(mods: &[Mod], order: &[usize]) {
    println!(
        "{}",
        if order.is_empty() {
            "No active mods."
        } else {
            "Load order (later wins):"
        }
    );
    for (n, &index) in order.iter().enumerate() {
        let found = &mods[index];
        println!(
            "  {}. {} {}  (priority {})  {}",
            n + 1,
            found.id,
            found.version,
            found.priority,
            found.name
        );
    }
    for found in mods.iter().filter(|m| !m.active()) {
        let reasons: Vec<String> = found.errors.iter().map(ToString::to_string).collect();
        println!(
            "  - {} {} NOT LOADED: {}",
            found.id,
            found.version,
            reasons.join("; ")
        );
    }
}

fn run(args: Args) -> Result<(), String> {
    let mods_dir = args.mods.ok_or("--mods is required")?;
    if args.command == "import" {
        let zip = args.zip.ok_or("--zip is required")?;
        let imported = mods::import_zip(&zip, &mods_dir).map_err(|e| e.to_string())?;
        match imported.previous {
            Some(previous) => println!(
                "Replaced {} {previous} with {}",
                imported.id, imported.version
            ),
            None => println!("Added {} {}", imported.id, imported.version),
        }
        return Ok(());
    }
    let channel = args.channel.as_deref().unwrap_or(DEFAULT_CHANNEL);
    let game = game::find(args.game_dir.as_deref(), channel).map_err(|e| e.to_string())?;
    let mut mods = mods::load(&mods_dir);
    print_game(&game);
    match args.command.as_str() {
        "status" => {
            let order = mods::resolve(&mut mods, &game.release);
            print_mods(&mods, &order);
        }
        "plan" => {
            let out = args.out.ok_or("--out is required")?;
            let Build {
                plan,
                report,
                order,
                tallies,
            } = features::build(&game, &mut mods, &out).map_err(|e| e.to_string())?;
            print_mods(&mods, &order);
            print!("{report}");
            for tally in &tallies {
                println!(
                    "{}: {} of {} {} patch(es)",
                    tally.backend.dll_name(),
                    tally.planned,
                    tally.wanted,
                    tally.feature
                );
            }
            println!("File overrides: {}", plan.redirects.len());
            let path = out.join(PLAN_NAME);
            plan.write(&path).map_err(|e| e.to_string())?;
            println!("Plan written to {}", path.display());
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
