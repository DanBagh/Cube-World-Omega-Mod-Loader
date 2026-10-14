//! Sounds a mod adds: the `"sounds"` manifest section.
//!
//! The client's sound table has a fixed size, and every slot in it is the client's. An added
//! sound gets an id from 8000h on, which the client's own code never uses, and the in-game
//! loader keeps a slot of its own for it. Once the client has loaded its sounds, the loader
//! loads each added one the same way. When the client is asked to play an id from 8000h on,
//! the loader plays the added sound itself, with the client's own functions. `pickupSounds`
//! and `soundPitch` name added sounds by those ids.
//!
//! Build-specific: the client's load and play functions are found by their shape, and the
//! functions they call are read from them. A client without them plays no added sounds.

use std::collections::BTreeMap;

use regex::bytes::Regex;

use super::ModuleBuild;
use super::files::Override;
use super::sounds::Added;
use super::stub::{CLIENT, DATA, RETURN, Stub, i32_at, signature};
use crate::mods::Mod;
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::Report;

/// The id the first added sound plays under.
pub const FIRST: u16 = 0x8000;
/// The most sounds the mods can add together.
const MAX: usize = 32;
/// Room for each path after the load trampoline, its end included.
const PATH: usize = 64;
/// Where the slots start in the data area. Other features keep their records below.
const SLOTS: u32 = 0x1000;
/// Voices per added sound, as many as the client gives each of its own.
const VOICES: u32 = 4;
/// Fewer calls to the client's load function than this is not its sound loader.
const MIN_LOADS: usize = 32;
/// How far into the load and play functions their parts are looked for.
const BODY: usize = 0x400;

// movzx r12d,dx / imul rax,r12,<size>: load(manager, id, path) finds the id's sound
const LOAD_INDEX: &str = r"\x44\x0f\xb7\xe2\x49\x69\xc4(....)";
// lea r15,[rcx+<engine>]: the audio engine, inside the manager
const ENGINE: &str = r"\x4c\x8d\xb9(....)";
// mov r8d,<flags> / xor r9d,r9d / call init_from_file
const INIT_FROM_FILE: &str = r"\x41\xb8(....)\x45\x31\xc9\xe8(....)";
// xor r8d,r8d / xor r9d,r9d / call init_copy: a voice of the sound
const INIT_COPY: &str = r"\x45\x31\xc0\x45\x31\xc9\xe8(....)";
// movzx eax,word [rdx] / cmp rax,<table size> / je: play(manager, entry) checks the id
const CHECK: &str = r"\x0f\xb7\x02\x48\x3d....\x0f\x84";
// imul rcx,r15,<size> / add rcx,r14 / add rcx,rax / call is_playing: a voice is tried
const IS_PLAYING: &str = r"\x49\x69\xcf(....)\x4c\x01\xf1\x48\x01\xc1\xe8(....)";
// mov rcx,r14 / call is_playing / test eax,eax / je / mov rcx,r14 / call stop
const STOP: &str = r"\x4c\x89\xf1\xe8(....)\x85\xc0\x74.\x4c\x89\xf1\xe8(....)";
// mov rcx,r14 / xor edx,edx / call seek
const SEEK: &str = r"\x4c\x89\xf1\x31\xd2\xe8(....)";
// lea rcx,[rsi+<graph>] / movzx r8d,byte [rdi+<spatial>] / mov rdx,r14 / call attach
const ATTACH: &str = r"\x48\x8d\x8e(....)\x44\x0f\xb6\x47.\x4c\x89\xf2\xe8(....)";
// movzx edx,byte [rdi+<spatial>] / mov rcx,r14 / call set_spatialization
const SPATIAL: &str = r"\x0f\xb6\x57.\x4c\x89\xf1\xe8(....)";
// (v)movss xmm1,[rdi+<pitch>] / mov rcx,r14 / call set_pitch
const PITCH: &str = r"(?:\xc5\xfa|\xf3\x0f)\x10\x4f(.)\x4c\x89\xf1\xe8(....)";
// (v)mulss xmm,[rsi+rcx*4+<volumes>] / mov rcx,r14 / [movaps xmm1,..] / call set_volume
const VOLUME: &str = r"\x59[\x80-\xbf]\x8e(....)\x4c\x89\xf1(?:\x0f\x28[\xc8-\xcf])?\xe8(....)";
// mov ecx,<category> / cmp ax,..: the category of every sound but a few
const CATEGORY: &str = r"\xb9(....)\x66\x83\xf8";
// mov rcx,r14 / call start / inc ebx
const START: &str = r"\x4c\x89\xf1\xe8(....)\xff\xc3";

// asm/sound_load.asm
pub(super) const LOAD: &str = "535657415441554883ec38488bd9e8cccccccc4c8d250000000033f681fe111111110f83880000004869fe\
12121212488d05eeeeeeee4803f8488d8b14141414486bd6404903d441b8151515154533c948c744242000000000\
488d47104889442428e8c1c1c1c185c0753e4533ed488d8b14141414488d57104533c04533c9418d45014869c013\
131313488d4407104889442420e8c2c2c2c285c0750941ffc54183fd0472c844892fffc6e96cffffff4883c43841\
5d415c5f5e5be9dddddddd";
// asm/sound_play.asm
pub(super) const PLAY: &str = "66813a0080730e909090909090909090e9dddddddd488bf1488bfa0fb71f81eb0080000081fb111111110f83\
e20000004869db121212124c8d2deeeeeeee4903dd448b234585e40f84c5000000448b7b04453bfc72034533ff45\
8bec418d47014869c813131313488d4c0b10e8c1c1c1c185c0741041ffc7453bfc72034533ff41ffcd75d7418d47\
014c69f0131313134e8d743310498bcee8c1c1c1c185c07408498bcee8c2c2c2c2498bce33d2e8c3c3c3c3488d8e\
14141414498bd64533c0e8c4c4c4c485c07549498bce33d2e8c5c5c5c5f30f108f15151515498bcee8c6c6c6c6f3\
0f108f16161616f30f598e17171717498bcee8c7c7c7c7498bcee8c8c8c8c841ffc7453bfc72034533ff44897b04\
3bc0e9dddddddd";

/// One sound a mod adds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedSound {
    pub name: String,
    pub id: u16,
    /// The game path its file is served under.
    pub path: String,
    pub owner: String,
}

/// What `pickupSounds` and `soundPitch` are given for one client.
pub fn names(added: &[AddedSound], playable: bool) -> Added {
    Added {
        ids: added
            .iter()
            .map(|sound| (sound.name.clone(), sound.id))
            .collect(),
        playable,
    }
}

fn valid_name(name: &str) -> bool {
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-');
    (1..=40).contains(&name.len())
        && name.chars().all(allowed)
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
}

/// The sounds the mods add, each with an id and its file served under a game path of its
/// own. When two mods add a sound of the same name, the later wins.
pub fn collect(
    mods: &[Mod],
    order: &[usize],
    overrides: &mut BTreeMap<String, Override>,
    report: &mut Report,
) -> Vec<AddedSound> {
    let mut wanted: Vec<(String, Override)> = Vec::new();
    for found in order.iter().map(|&index| &mods[index]) {
        let id = &found.id;
        for (name, source) in found.object("sounds").into_iter().flatten() {
            let relative = source.as_str().unwrap_or_default();
            let file = found.file(relative).filter(|path| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
            });
            if !valid_name(name) {
                report.warn(format!("{id}: sound name '{name}' must be 1 to 40 lowercase letters, digits, '.', '_' or '-'; skipped"));
                continue;
            }
            let Some(source) = file else {
                report.warn(format!(
                    "{id}: sound '{relative}' must be a .wav file inside the mod folder; skipped"
                ));
                continue;
            };
            let sound = Override {
                source,
                owner: id.clone(),
            };
            match wanted.iter_mut().find(|(known, _)| known == name) {
                Some((_, earlier)) => {
                    if earlier.owner != *id {
                        report.warn(format!(
                            "sound '{name}' is defined by {} and {id}; {id} wins",
                            earlier.owner
                        ));
                    }
                    *earlier = sound;
                }
                None => wanted.push((name.clone(), sound)),
            }
        }
    }
    let mut added = Vec::new();
    for (name, sound) in wanted {
        if added.len() == MAX {
            report.warn(format!(
                "{}: more than {MAX} added sounds; '{name}' skipped",
                sound.owner
            ));
            continue;
        }
        let id = FIRST + added.len() as u16;
        let path = format!("sounds/cwoml-{name}.wav");
        report.say(format!(
            "Added sound '{name}' ({}) plays as id {id:#x}",
            sound.owner
        ));
        let owner = sound.owner.clone();
        overrides.insert(path.clone(), sound);
        added.push(AddedSound {
            name,
            id,
            path,
            owner,
        });
    }
    added
}

/// Where a `call rel32` at `at` in `text` goes.
fn target(text_va: u32, text: &[u8], at: usize) -> u32 {
    (text_va + at as u32 + 5).wrapping_add_signed(i32_at(text, at + 1))
}

/// The first match of `pattern` in `text[from..until]`, with the offsets of its groups in
/// `text`.
fn find<'h>(
    pattern: &Regex,
    text: &'h [u8],
    from: usize,
    until: usize,
) -> Option<regex::bytes::Captures<'h>> {
    pattern.captures_at(&text[..until.min(text.len())], from)
}

/// The client's load(manager, id, path), what it calls, and its last call.
struct Load {
    /// The last call to load in the sound loader.
    site: u32,
    expect: Vec<u8>,
    load: u32,
    init_from_file: u32,
    init_copy: u32,
    /// Where the manager keeps its audio engine.
    engine: u32,
    /// What the client loads its sounds with.
    flags: u32,
    /// The size of one sound.
    size: u32,
}

fn find_load(module: &ModuleBuild) -> Option<Load> {
    let (text_va, text) = module.image.data(".text")?;
    let index = signature(LOAD_INDEX);
    let mut hits = index.captures_iter(text);
    let hit = hits.next()?;
    if hits.next().is_some() {
        return None;
    }
    let size = u32::try_from(i32_at(&hit[1], 0)).ok()?;
    let load = module
        .image
        .function_start(text_va + hit.get(0)?.start() as u32)?;
    let body = (load - text_va) as usize;
    let end = body + BODY;
    let engine = find(&signature(ENGINE), text, body, end)?;
    let from_file = find(&signature(INIT_FROM_FILE), text, body, end)?;
    let flags = i32_at(&from_file[1], 0) as u32;
    let call = from_file.get(2)?.start() - 1;
    let init_from_file = target(text_va, text, call);
    let copy = find(&signature(INIT_COPY), text, call, end)?;
    let init_copy = target(text_va, text, copy.get(1)?.start() - 1);
    // every call to load: the last one ends the loading of the client's own sounds
    let calls: Vec<usize> = memchr::memchr_iter(0xE8, text)
        .filter(|&at| at + 5 <= text.len() && target(text_va, text, at) == load)
        .collect();
    let &last = calls.last()?;
    // mov dx,<id> in front: it loads one of the numbered sounds
    let numbered = memchr::memmem::find(&text[last.saturating_sub(16)..last], &[0x66, 0xBA]);
    if calls.len() < MIN_LOADS || numbered.is_none() {
        return None;
    }
    Some(Load {
        site: text_va + last as u32,
        expect: text[last..last + 5].to_vec(),
        load,
        init_from_file,
        init_copy,
        engine: i32_at(&engine[1], 0) as u32,
        flags,
        size,
    })
}

/// The client's play(manager, entry) and what it calls.
struct Play {
    site: u32,
    /// The two instructions that check the id.
    expect: Vec<u8>,
    /// is_playing, stop, seek, attach, set_spatialization, set_pitch, set_volume, start.
    calls: [u32; 8],
    /// What the manager attaches voices to.
    graph: u32,
    /// Where a queue entry keeps its pitch, and then its volume.
    pitch: u8,
    /// The manager's volume for the category of most sounds.
    volume: u32,
}

fn find_play(module: &ModuleBuild, size: u32) -> Option<Play> {
    let (text_va, text) = module.image.data(".text")?;
    let check = signature(CHECK);
    let mut hits = check.find_iter(text);
    let site = hits.next()?.start();
    if hits.next().is_some() {
        return None;
    }
    let end = site + BODY;
    let call = |hit: &regex::bytes::Captures, group: usize| {
        hit.get(group)
            .map(|rel| target(text_va, text, rel.start() - 1))
    };
    let tried = find(&signature(IS_PLAYING), text, site, end)?;
    if i32_at(&tried[1], 0) as u32 != size {
        return None;
    }
    let is_playing = call(&tried, 2)?;
    let from = tried.get(0)?.end();
    let stopped = find(&signature(STOP), text, from, end)?;
    if call(&stopped, 1)? != is_playing {
        return None;
    }
    let stop = call(&stopped, 2)?;
    let from = stopped.get(0)?.end();
    let seek = find(&signature(SEEK), text, from, end)?;
    let attach = find(&signature(ATTACH), text, seek.get(0)?.end(), end)?;
    let from = attach.get(0)?.end();
    let spatial = find(&signature(SPATIAL), text, from, end)?;
    let pitch = find(&signature(PITCH), text, spatial.get(0)?.end(), end)?;
    let from = pitch.get(0)?.end();
    let volume = find(&signature(VOLUME), text, from, end)?;
    let category = find(&signature(CATEGORY), text, from, end)?;
    let start = find(&signature(START), text, volume.get(0)?.end(), end)?;
    // the entry's volume comes right after its pitch: (v)movss xmm,[rdx+<volume>] early on
    let offset = pitch[1][0];
    let volume_at = offset.wrapping_add(4);
    let reads_volume = text[site..site + 0x40]
        .windows(3)
        .any(|code| code[0] == 0x10 && code[1] & 0xC7 == 0x42 && code[2] == volume_at);
    if !reads_volume {
        return None;
    }
    let category = i32_at(&category[1], 0) as u32;
    Some(Play {
        site: text_va + site as u32,
        expect: text[site..site + 9].to_vec(),
        calls: [
            is_playing,
            stop,
            call(&seek, 1)?,
            call(&attach, 2)?,
            call(&spatial, 1)?,
            call(&pitch, 2)?,
            call(&volume, 2)?,
            call(&start, 1)?,
        ],
        graph: i32_at(&attach[1], 0) as u32,
        pitch: offset,
        volume: (i32_at(&volume[1], 0) as u32).wrapping_add(category * 4),
    })
}

/// Plans the loading and playing of the added sounds. False if this client has no way to
/// play them, in which case nothing may name them.
pub fn plan(module: &mut ModuleBuild, added: &[AddedSound], report: &mut Report) -> bool {
    if added.is_empty() {
        return true;
    }
    let backend = module.backend.key();
    let found = find_load(module).and_then(|load| Some((find_play(module, load.size)?, load)));
    let Some((play, load)) = found else {
        report.warn(format!(
            "{backend}: could not locate how the client loads and plays its sounds in this build; added sounds skipped"
        ));
        return false;
    };
    let slot = (0x10 + (VOICES + 1) * load.size).next_multiple_of(16);
    let count = added.len() as i32;
    let size = load.size as i32;
    let mut ops = Vec::new();
    let calls = |stub: &Stub, start: u32, marker: u8, target: u32, ops: &mut Vec<Op>| {
        for at in stub.all(marker) {
            ops.push(Op::BlobRel32 {
                blob_offset: start + at,
                target,
            });
        }
    };

    // after the client's last load: load the added sounds
    let mut stub = Stub::new(LOAD);
    stub.fill(0x11, count);
    stub.fill(0x12, slot as i32);
    stub.fill(0x13, size);
    stub.fill(0x14, load.engine as i32);
    stub.fill(0x15, load.flags as i32);
    stub.lea([0x4C, 0x8D, 0x25], 0, stub.size()); // the paths follow the code
    let start = module.plan.reserve();
    for (marker, client) in [
        (CLIENT, load.load),
        (0xC1, load.init_from_file),
        (0xC2, load.init_copy),
    ] {
        calls(&stub, start, marker, client, &mut ops);
    }
    calls(&stub, start, RETURN, load.site + 5, &mut ops);
    module.plan.data_ref(start + stub.at(DATA), SLOTS);
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    for sound in added {
        let mut path = sound.path.as_bytes().to_vec();
        path.resize(PATH, 0);
        blob.extend(path);
    }
    blob.resize(blob.len() + BLOB_PAD, 0);
    ops.push(Op::Hook {
        rva: load.site,
        expect: load.expect.clone(),
        blob_offset: start,
    });

    // at the start of the client's play: play an added sound from its slot
    let mut stub = Stub::new(PLAY);
    stub.fill(0x11, count);
    stub.fill(0x12, slot as i32);
    stub.fill(0x13, size);
    stub.fill(0x14, play.graph as i32);
    stub.fill(0x15, i32::from(play.pitch));
    stub.fill(0x16, i32::from(play.pitch) + 4);
    stub.fill(0x17, play.volume as i32);
    let start = module.plan.reserve();
    for (marker, client) in (0xC1..=0xC8).zip(play.calls) {
        calls(&stub, start, marker, client, &mut ops);
    }
    calls(&stub, start, RETURN, play.site + 9, &mut ops);
    module.plan.data_ref(start + stub.at(DATA), SLOTS);
    let mut code = stub.into_bytes();
    let replaced = code
        .windows(9)
        .position(|bytes| bytes == [0x90; 9])
        .expect("room for the replaced instructions");
    code[replaced..replaced + 9].copy_from_slice(&play.expect);
    let blob = &mut module.plan.blob;
    blob.extend(code);
    blob.resize(blob.len() + BLOB_PAD, 0);
    ops.push(Op::Hook {
        rva: play.site,
        expect: play.expect.clone(),
        blob_offset: start,
    });

    let data_end = SLOTS + slot * added.len() as u32;
    module.plan.data_size = module.plan.data_size.max(data_end);
    for (site, length, owner) in [
        (load.site, 5, "added sounds: loading"),
        (play.site, 9, "added sounds: playing"),
    ] {
        module.plan.claims.push(Claim {
            start: site,
            end: site + length,
            owner: owner.into(),
        });
    }
    let mut who: Vec<&str> = Vec::new();
    for sound in added {
        if !who.contains(&sound.owner.as_str()) {
            who.push(&sound.owner);
        }
    }
    module.plan.groups.push(Group {
        id: "added-sounds".into(),
        label: format!("{} added sound(s) [{}]", added.len(), who.join("+")),
        ops,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Backend;
    use crate::mods::load;
    use crate::mods::tests::write_mod;
    use crate::pe::PeImage;
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    #[test]
    fn added_sounds_get_ids_and_names_of_their_own() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, manifest) in [
            (
                "a",
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "sounds": {
                    "chime": "chime.wav", "Bad Name": "chime.wav", "text": "notes.txt", "gone": "gone.wav"}}"#,
            ),
            (
                "b",
                r#"{"schema": 1, "id": "b", "version": "1.0.0", "sounds": {
                    "chime": "other.wav", "second": "other.wav"}}"#,
            ),
        ] {
            let folder = write_mod(dir.path(), folder, manifest);
            for file in ["chime.wav", "other.wav", "notes.txt"] {
                std::fs::write(folder.join(file), b"RIFF").unwrap();
            }
        }
        let mods = load(dir.path());
        let mut overrides = BTreeMap::new();
        let mut report = Report::default();
        let added = collect(&mods, &[0, 1], &mut overrides, &mut report);

        let brief: Vec<(&str, u16, &str)> = added
            .iter()
            .map(|s| (s.name.as_str(), s.id, s.path.as_str()))
            .collect();
        // in the order the names were first seen. b's chime replaced a's
        assert_eq!(
            brief,
            [
                ("chime", 0x8000, "sounds/cwoml-chime.wav"),
                ("second", 0x8001, "sounds/cwoml-second.wav"),
            ]
        );
        let chime = &overrides["sounds/cwoml-chime.wav"];
        assert!(chime.owner == "b" && chime.source.ends_with("other.wav"));
        let warnings: Vec<&str> = report.warnings().collect();
        assert_eq!(warnings.len(), 4);
        assert!(warnings[0].contains("sound name 'Bad Name' must be"));
        assert!(warnings[3].contains("is defined by a and b; b wins"));
        assert_eq!(
            names(&added, false),
            Added {
                ids: BTreeMap::from([("chime".to_owned(), 0x8000), ("second".to_owned(), 0x8001)]),
                playable: false,
            }
        );
    }

    /// A client function that does nothing, with an entry in the exception directory.
    fn function(pe: &mut TinyPe) -> u32 {
        let begin = pe.code(&[0xC3]);
        pe.function(begin, begin + 1);
        begin
    }

    /// Writes `call target` at the end of the code.
    fn call(pe: &mut TinyPe, target: u32) -> u32 {
        let at = pe.code(&[0xE8, 0, 0, 0, 0]);
        pe.patch(at + 1, &target.wrapping_sub(at + 5).to_le_bytes());
        at
    }

    /// The client's sound code, the way game 0.1.0 has it: load, many calls to it, and
    /// play. Returns the client, load, the last call to it, the start of play's check and
    /// the functions they call.
    fn client() -> (PeImage, u32, u32, u32, [u32; 10]) {
        let mut pe = TinyPe::new();
        let called: [u32; 10] = std::array::from_fn(|_| function(&mut pe));
        let [
            init_from_file,
            init_copy,
            is_playing,
            stop,
            seek,
            attach,
            spatial,
            pitch,
            volume,
            start,
        ] = called;
        // load(manager, id, path)
        let load = pe.code(&[0x56, 0x57, 0x48, 0x83, 0xEC, 0x30]);
        pe.code(&[0x44, 0x0F, 0xB7, 0xE2, 0x49, 0x69, 0xC4, 0xB8, 3, 0, 0]);
        pe.code(&[0x4C, 0x8D, 0xB9, 0x00, 0x3E, 0x73, 0x0E]);
        pe.code(&[0x41, 0xB8, 0x02, 0x10, 0, 0, 0x45, 0x31, 0xC9]);
        call(&mut pe, init_from_file);
        pe.code(&[0x90; 8]);
        pe.code(&[0x45, 0x31, 0xC0, 0x45, 0x31, 0xC9]);
        call(&mut pe, init_copy);
        let end = pe.code(&[0x48, 0x83, 0xC4, 0x30, 0x5F, 0x5E, 0xC3]);
        pe.function(load, end + 7);
        // the sound loader: mov rcx,rsi / mov dx,<id> / call load, for every sound
        let mut last = 0;
        for id in 1..=MIN_LOADS as u16 {
            pe.code(&[0x48, 0x89, 0xF1]);
            pe.code(&[&[0x66, 0xBA][..], &id.to_le_bytes()].concat());
            last = call(&mut pe, load);
        }
        pe.code(&[0xC3]);
        // play(manager, entry)
        pe.code(&[0x56, 0x57, 0x48, 0x83, 0xEC, 0x68]);
        let check = pe.code(&[
            0x0F, 0xB7, 0x02, 0x48, 0x3D, 0x8A, 0, 0, 0, 0x0F, 0x84, 0, 1, 0, 0,
        ]);
        pe.code(&[0xC5, 0x7A, 0x10, 0x4A, 0x24, 0x80, 0x7A, 0x28, 0x01]);
        pe.code(&[
            0x49, 0x69, 0xCF, 0xB8, 3, 0, 0, 0x4C, 0x01, 0xF1, 0x48, 0x01, 0xC1,
        ]);
        call(&mut pe, is_playing);
        pe.code(&[0x85, 0xC0, 0x74, 0x09, 0x90, 0x90]);
        pe.code(&[0x4C, 0x89, 0xF1]);
        call(&mut pe, is_playing);
        pe.code(&[0x85, 0xC0, 0x74, 0x08, 0x4C, 0x89, 0xF1]);
        call(&mut pe, stop);
        pe.code(&[0x4C, 0x89, 0xF1, 0x31, 0xD2]);
        call(&mut pe, seek);
        pe.code(&[
            0x48, 0x8D, 0x8E, 0xE8, 0x46, 0x73, 0x0E, 0x44, 0x0F, 0xB6, 0x47, 0x28,
        ]);
        pe.code(&[0x4C, 0x89, 0xF2]);
        call(&mut pe, attach);
        pe.code(&[0x0F, 0xB6, 0x57, 0x28, 0x4C, 0x89, 0xF1]);
        call(&mut pe, spatial);
        pe.code(&[0xC5, 0xFA, 0x10, 0x4F, 0x20, 0x4C, 0x89, 0xF1]);
        call(&mut pe, pitch);
        pe.code(&[0xB9, 3, 0, 0, 0, 0x66, 0x83, 0xF8, 0x17]);
        pe.code(&[
            0xC5, 0xB2, 0x59, 0x8C, 0x8E, 0xE0, 0x38, 0x73, 0x0E, 0x4C, 0x89, 0xF1,
        ]);
        call(&mut pe, volume);
        pe.code(&[0x4C, 0x89, 0xF1]);
        call(&mut pe, start);
        pe.code(&[0xFF, 0xC3, 0xC3]);
        pe.code(&[0x90; 0x80]);
        (pe.build(), load, last, check, called)
    }

    fn module<'a>(image: &'a PeImage, refs: &'a RefIndex) -> ModuleBuild<'a> {
        ModuleBuild {
            backend: Backend::Avx2,
            image,
            refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        }
    }

    #[test]
    fn loads_and_plays_added_sounds_with_the_clients_own_functions() {
        let (image, load, last, check, called) = client();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = module(&image, &refs);
        let added = [AddedSound {
            name: "chime".into(),
            id: FIRST,
            path: "sounds/cwoml-chime.wav".into(),
            owner: "a".into(),
        }];
        assert!(plan(&mut module, &added, &mut Report::default()));

        let group = &module.plan.groups[0];
        assert_eq!(group.label, "1 added sound(s) [a]");
        let hooks: Vec<(u32, &[u8])> = group
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Hook { rva, expect, .. } => Some((*rva, &expect[..])),
                _ => None,
            })
            .collect();
        let check_bytes = [0x0F, 0xB7, 0x02, 0x48, 0x3D, 0x8A, 0, 0, 0];
        let call_bytes = image.read(last, 5).unwrap();
        assert_eq!(hooks, [(last, call_bytes), (check, &check_bytes[..])]);
        // every call the trampolines make goes where the client's own do
        let targets: Vec<u32> = group
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::BlobRel32 { target, .. } => Some(*target),
                _ => None,
            })
            .collect();
        let [
            init_from_file,
            init_copy,
            is_playing,
            stop,
            seek,
            attach,
            spatial,
            pitch,
            volume,
            start,
        ] = called;
        let wanted = [
            load,
            init_from_file,
            init_copy,
            last + 5,
            is_playing,
            is_playing,
            stop,
            seek,
            attach,
            spatial,
            pitch,
            volume,
            start,
            check + 9,
            check + 9,
        ];
        assert_eq!(targets, wanted);
        // the play trampoline makes the replaced check itself, the path follows the loader
        let blob = &module.plan.blob;
        assert!(blob.windows(9).any(|bytes| bytes == check_bytes));
        assert!(
            blob.windows(22)
                .any(|bytes| bytes == b"sounds/cwoml-chime.wav")
        );
        let slot = (0x10 + 5 * 0x3B8u32).next_multiple_of(16);
        assert_eq!(module.plan.data_size, SLOTS + slot);
    }

    #[test]
    fn a_client_without_the_sound_code_plays_no_added_sounds() {
        let mut pe = TinyPe::new();
        pe.code(&[0x90; 64]);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = module(&image, &refs);
        let added = [AddedSound {
            name: "chime".into(),
            id: FIRST,
            path: "sounds/cwoml-chime.wav".into(),
            owner: "a".into(),
        }];
        let mut report = Report::default();
        assert!(!plan(&mut module, &added, &mut report));
        assert!(module.plan.groups.is_empty());
        assert!(
            report
                .warnings()
                .next()
                .unwrap()
                .starts_with("avx2: could not locate")
        );
        assert!(plan(&mut module, &[], &mut report));
    }
}
