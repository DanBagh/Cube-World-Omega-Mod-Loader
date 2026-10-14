//! Sound pitch: the `"soundPitch"` manifest section.
//!
//! The pitch of a sound effect is chosen where it is queued, and some of those places
//! speed a sound up a lot. Queued sounds are played once a frame from one loop. That call
//! is diverted, so the sounds in the table get a pitch from their own range just before
//! they play. A later mod replaces an earlier one's choice for the same sound.
//!
//! Build-specific: the loop is found by its instructions.

use memchr::memmem;
use serde_json::Value;

use super::ModuleBuild;
use super::sounds::{self, Added};
use super::stub::{CLIENT, RETURN, Stub, i32_at, only};
use crate::mods::{Mod, Notice};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::Report;

/// The trampoline picks one of 1024 pitches between the lowest and the highest.
const STEPS: f64 = 1023.0;
const LIMITS: (f64, f64) = (0.25, 4.0);

// mov rcx,rsi / mov rdx,rbx / call play_entry, then add rbx,38h / dec rdi / jnz
const SITE: &str = r"(\x48\x89\xf1\x48\x89\xda\xe8(....))\x48\x83\xc3\x38\x48\xff\xcf\x75\xec";
// How queueing a sound ends: a float stored at +20h, then 1.0 at +24h. It is what says
// the pitch is the float at +20h
const LAYOUT: [u8; 10] = [0x01, 0x20, 0xC7, 0x44, 0x01, 0x24, 0x00, 0x00, 0x80, 0x3F];
// asm/pitch_hook.asm
pub(super) const STUB: &str = "440fb7034c8d0d00000000410fb70981f9ffff00007427413bc874064983c10cebe90f3125ff030000f30f2a\
c0f3410f594108f3410f584104f30f114320488bce488bd3e8cccccccce9dddddddd";

#[derive(Debug, Clone, PartialEq)]
pub struct Pitch {
    /// A game path, or the name of a sound a mod adds.
    pub sound: String,
    pub low: f64,
    pub high: f64,
    pub owner: String,
}

fn range(value: &Value) -> Option<(f64, f64)> {
    let (low, high) = match value.as_array().map(Vec::as_slice) {
        Some([low, high]) => (low.as_f64()?, high.as_f64()?),
        Some(_) => return None,
        None => (value.as_f64()?, value.as_f64()?),
    };
    (LIMITS.0 <= low && low <= high && high <= LIMITS.1).then_some((low, high))
}

/// One pitch range per sound, in the order the sounds were first named.
pub fn collect(mods: &mut [Mod], order: &[usize], report: &mut Report) -> Vec<Pitch> {
    let mut chosen: Vec<Pitch> = Vec::new();
    for &index in order {
        let found = &mut mods[index];
        let patches = match found.manifest.get("soundPitch") {
            None | Some(Value::Null) => continue,
            Some(Value::Object(patches)) => patches.clone(),
            Some(_) => {
                found
                    .warnings
                    .push(Notice::new("Sound pitches ignored").with(
                        "Expected",
                        r#"an object like {"sounds/drop-coin.wav": 1.0}"#,
                    ));
                continue;
            }
        };
        for (name, value) in patches {
            let sound = name.replace('\\', "/");
            let Some((low, high)) = range(&value) else {
                found.warnings.push(
                    Notice::new("Sound pitch ignored")
                        .with("Sound", sound)
                        .with(
                            "Expected",
                            format!(
                                "a number from {} to {}, or [lowest, highest]",
                                LIMITS.0, LIMITS.1
                            ),
                        ),
                );
                continue;
            };
            let pitch = Pitch {
                sound,
                low,
                high,
                owner: found.id.clone(),
            };
            match chosen.iter_mut().find(|p| p.sound == pitch.sound) {
                Some(earlier) => {
                    if earlier.owner != pitch.owner {
                        report.warn(format!(
                            "sound pitch conflict for {}: {} and {}; {} wins",
                            pitch.sound, earlier.owner, pitch.owner, pitch.owner
                        ));
                    }
                    *earlier = pitch;
                }
                None => chosen.push(pitch),
            }
        }
    }
    chosen
}

struct Hook {
    site: u32,
    expect: Vec<u8>,
    /// The function the replaced call goes to.
    play: u32,
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let (text_va, text) = module.image.data(".text")?;
    let hit = only(SITE, text)?;
    memmem::find(text, &LAYOUT)?;
    let replaced = hit.get(1)?;
    let site = text_va + replaced.start() as u32;
    let resume = site + replaced.len() as u32;
    Some(Hook {
        site,
        expect: replaced.as_bytes().to_vec(),
        play: resume.wrapping_add_signed(i32_at(&hit[2], 0)),
    })
}

/// Plans the pitches whose sound this client has. Returns how many that was.
pub fn plan(
    module: &mut ModuleBuild,
    pitches: &[Pitch],
    added: &Added,
    report: &mut Report,
) -> usize {
    if pitches.is_empty() {
        return 0;
    }
    let backend = module.backend.key();
    let Some(hook) = find_hook(module) else {
        report.warn(format!("{backend}: could not locate the code that plays queued sounds in this build; sound pitches skipped"));
        return 0;
    };
    let mut table = Vec::new();
    let mut who: Vec<&str> = Vec::new();
    for pitch in pitches {
        let Some(id) = sounds::resolve(module.image, module.refs, added, &pitch.sound) else {
            report.warn(format!(
                "{backend}: {}: soundPitch: '{}' {}; entry skipped",
                pitch.owner,
                pitch.sound,
                sounds::unknown(&pitch.sound)
            ));
            continue;
        };
        table.extend_from_slice(&id.to_le_bytes());
        table.extend_from_slice(&[0, 0]);
        table.extend_from_slice(&(pitch.low as f32).to_le_bytes());
        table.extend_from_slice(&(((pitch.high - pitch.low) / STEPS) as f32).to_le_bytes());
        if !who.contains(&pitch.owner.as_str()) {
            who.push(&pitch.owner);
        }
    }
    let planned = table.len() / 12;
    if planned == 0 {
        return 0;
    }
    table.extend_from_slice(&[0xFF, 0xFF]);

    let mut stub = Stub::new(STUB);
    stub.lea([0x4C, 0x8D, 0x0D], 0, stub.size()); // the table follows the code
    let (play, resume) = (stub.at(CLIENT), stub.at(RETURN));
    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    blob.extend(table);
    blob.resize(blob.len() + BLOB_PAD, 0);

    let end = hook.site + hook.expect.len() as u32;
    module.plan.claims.push(Claim {
        start: hook.site,
        end,
        owner: "sound pitches".into(),
    });
    module.plan.groups.push(Group {
        id: "sound-pitch".into(),
        label: format!("{planned} sound pitch(es) [{}]", who.join("+")),
        ops: vec![
            Op::BlobRel32 {
                blob_offset: start + play,
                target: hook.play,
            },
            Op::BlobRel32 {
                blob_offset: start + resume,
                target: end,
            },
            Op::Hook {
                rva: hook.site,
                expect: hook.expect,
                blob_offset: start,
            },
        ],
    });
    planned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sounds::tests::loads;
    use crate::game::Backend;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    fn pitch(sound: &str, low: f64, high: f64, owner: &str) -> Pitch {
        Pitch {
            sound: sound.into(),
            low,
            high,
            owner: owner.into(),
        }
    }

    #[test]
    fn a_pitch_is_one_number_or_a_range() {
        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "a", "version": "1.0.0", "soundPitch": {
                "sounds\\drop-coin.wav": 1.0, "chime": [0.9, 1.1], "low": 0.1,
                "backwards": [1.2, 0.8], "word": "fast", "three": [1, 1, 1]}}"#,
        );
        write_mod(
            dir.path(),
            "b",
            r#"{"schema": 1, "id": "b", "version": "1.0.0", "soundPitch": {"chime": 2}}"#,
        );
        write_mod(
            dir.path(),
            "c",
            r#"{"schema": 1, "id": "c", "version": "1.0.0", "soundPitch": [1.0]}"#,
        );
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let pitches = collect(&mut mods, &order, &mut report);
        assert_eq!(
            pitches,
            [
                pitch("sounds/drop-coin.wav", 1.0, 1.0, "a"),
                pitch("chime", 2.0, 2.0, "b"),
            ]
        );
        assert_eq!(mods[0].warnings.len(), 4);
        assert_eq!(mods[2].warnings[0].title, "Sound pitches ignored");
        assert_eq!(
            report.warnings().collect::<Vec<_>>(),
            ["sound pitch conflict for chime: a and b; b wins"]
        );
    }

    #[test]
    fn hooks_the_call_that_plays_a_queued_sound() {
        let mut pe = TinyPe::new();
        loads(&mut pe, "sounds/drop-coin.wav", 0x4A);
        pe.code(&LAYOUT);
        pe.code(&[0x90; 16]);
        let site = pe.code(&[0x48, 0x89, 0xF1, 0x48, 0x89, 0xDA, 0xE8, 0x00, 0x02, 0, 0]);
        pe.code(&[0x48, 0x83, 0xC3, 0x38, 0x48, 0xFF, 0xCF, 0x75, 0xEC]);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = ModuleBuild {
            backend: Backend::Avx2,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        };
        let pitches = [
            pitch("sounds/drop-coin.wav", 0.5, 1.5, "a"),
            pitch("sounds/nope.wav", 1.0, 1.0, "b"),
        ];
        let mut report = Report::default();
        assert_eq!(plan(&mut module, &pitches, &Added::new(), &mut report), 1);

        let plan = &module.plan;
        let group = &plan.groups[0];
        assert_eq!(
            (group.id.as_str(), group.label.as_str()),
            ("sound-pitch", "1 sound pitch(es) [a]")
        );
        let Op::Hook {
            rva,
            expect,
            blob_offset: start,
        } = &group.ops[2]
        else {
            panic!("the hook comes last")
        };
        assert_eq!((*rva, expect.len()), (site, 11));
        let stub = Stub::new(STUB);
        assert_eq!(
            group.ops[..2],
            [
                Op::BlobRel32 {
                    blob_offset: start + stub.at(CLIENT),
                    target: site + 11 + 0x200
                },
                Op::BlobRel32 {
                    blob_offset: start + stub.at(RETURN),
                    target: site + 11
                },
            ]
        );
        let table = *start as usize + stub.size();
        assert_eq!(
            plan.blob[table..table + 14],
            [
                &[0x4A, 0, 0, 0][..],
                &0.5f32.to_le_bytes(),
                &(1.0f32 / 1023.0).to_le_bytes(),
                &[0xFF, 0xFF],
            ]
            .concat()
        );
        assert_eq!(
            report.warnings().collect::<Vec<_>>(),
            ["avx2: b: soundPitch: 'sounds/nope.wav' is not a sound the game loads; entry skipped"]
        );
    }
}
