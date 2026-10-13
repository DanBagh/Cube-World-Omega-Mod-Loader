//! Text patches: the `"text"` and `"textFiles"` manifest sections.
//!
//! The game ships no text files. Every string is a literal inside the client DLL, so a
//! text patch puts the new string in the plan's blob and repoints whatever referenced the
//! old one: `lea` instructions and pointers in data tables. That needs nothing specific
//! to a build beyond the string still being there.
//!
//! Not ported yet: short text the compiler wrote into code as a constant (the " G" after
//! a price).

use std::fs;

use serde_json::Value;

use super::ModuleBuild;
use crate::mods::{Mod, Notice};
use crate::plan::{Group, Op};
use crate::refs::{SiteKind, string_sites};
use crate::report::{Report, one_line};

/// What to do when the code also carries the original string's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fit {
    /// Refuse a replacement of another length.
    Strict,
    /// Pad with spaces or cut to the original length.
    Pad,
    /// Patch anyway.
    Force,
}

/// One original string and what the mods, together, turn it into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEntry {
    pub original: String,
    pub value: String,
    /// Form to use after a lone 1, where the game appends the text to a number.
    pub singular: Option<String>,
    pub fit: Fit,
    /// Mod id and operation, in the order they were applied.
    pub history: Vec<(String, &'static str)>,
    setters: Vec<String>,
}

fn patches(found: &mut Mod) -> Vec<Value> {
    let mut all = found.list("text").to_vec();
    let files: Vec<String> = found
        .list("textFiles")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    for relative in files {
        let Some(path) = found.file(&relative) else {
            found
                .warnings
                .push(Notice::new("Text file not found").with("File", relative));
            continue;
        };
        match fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        {
            Some(extra) => all.extend(extra),
            None => found.warnings.push(
                Notice::new("Unreadable text file")
                    .with("File", relative)
                    .with("Expected", "a JSON array of patches"),
            ),
        }
    }
    all
}

/// Folds every mod's text operations, in load order, into one final value per string.
///
/// Operations compose: one mod can append to a string another mod set. Two mods that both
/// set the same string are a real conflict. The later one wins and it is reported.
pub fn collect(mods: &mut [Mod], order: &[usize], report: &mut Report) -> Vec<TextEntry> {
    let mut entries: Vec<TextEntry> = Vec::new();
    for &index in order {
        let found = &mut mods[index];
        for patch in patches(found) {
            let text = |key: &str| patch.get(key).and_then(Value::as_str);
            let Some(original) = text("match").filter(|m| !m.is_empty()) else {
                found
                    .warnings
                    .push(Notice::new("Text patch without a match").with("Patch", &patch));
                continue;
            };
            let at = entries
                .iter()
                .position(|e| e.original == original)
                .unwrap_or_else(|| {
                    entries.push(TextEntry {
                        original: original.to_owned(),
                        value: original.to_owned(),
                        singular: None,
                        fit: Fit::Strict,
                        history: Vec::new(),
                        setters: Vec::new(),
                    });
                    entries.len() - 1
                });
            let entry = &mut entries[at];
            let replace = patch
                .get("replace")
                .and_then(Value::as_object)
                .and_then(|r| {
                    Some((
                        r.get("from")?.as_str()?,
                        r.get("to").and_then(Value::as_str).unwrap_or_default(),
                    ))
                });
            let operation = if let Some(value) = text("set") {
                entry.value = value.to_owned();
                entry.singular = text("singular").map(str::to_owned);
                entry.setters.push(found.id.clone());
                "set"
            } else if let Some(suffix) = text("append") {
                entry.value.push_str(suffix);
                entry.singular = entry.singular.take().map(|s| s + suffix);
                "append"
            } else if let Some(prefix) = text("prepend") {
                entry.value.insert_str(0, prefix);
                entry.singular = entry.singular.take().map(|s| prefix.to_owned() + &s);
                "prepend"
            } else if let Some((from, to)) = replace {
                if !entry.value.contains(from) {
                    found.warnings.push(
                        Notice::new("Nothing to replace")
                            .with("Find", from)
                            .with("In", one_line(&entry.value, 70)),
                    );
                }
                entry.value = entry.value.replace(from, to);
                entry.singular = entry.singular.take().map(|s| s.replace(from, to));
                "replace"
            } else {
                found.warnings.push(
                    Notice::new("Text patch without a change")
                        .with("Text", one_line(original, 70))
                        .with("Needs", "set, append, prepend or replace"),
                );
                continue;
            };
            let fit = match text("fit") {
                None | Some("strict") => Fit::Strict,
                Some("pad") => Fit::Pad,
                Some("force") => Fit::Force,
                Some(other) => {
                    found.warnings.push(
                        Notice::new("Unknown fit")
                            .with("Text", one_line(original, 70))
                            .with("Value", other)
                            .with("Using", "strict"),
                    );
                    Fit::Strict
                }
            };
            entry.fit = entry.fit.max(fit);
            if operation != "set" && patch.get("singular").is_some() {
                found.warnings.push(
                    Notice::new("Singular ignored")
                        .with("Text", one_line(original, 70))
                        .with("Needs", "set"),
                );
            }
            entry.history.push((found.id.clone(), operation));
        }
    }
    for entry in &entries {
        let mut setters = entry.setters.clone();
        setters.dedup();
        if setters.iter().any(|s| s != &setters[0]) {
            report.warn(format!(
                "text conflict on \"{}\": set by {}; {} wins",
                one_line(&entry.original, 70),
                entry.setters.join(", "),
                entry.setters.last().unwrap()
            ));
        }
    }
    entries.retain(|e| e.value != e.original || e.singular.is_some());
    entries
}

fn latin1(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect()
}

fn group_id(original: &[u8]) -> String {
    // FNV-1a: the same string gets the same id in every build
    let hash = original.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("text:{:010x}", hash & 0xff_ffff_ffff)
}

/// Plans every entry it can for one client DLL. Returns how many that was.
pub fn plan(module: &mut ModuleBuild, entries: &[TextEntry], report: &mut Report) -> usize {
    entries
        .iter()
        .filter(|entry| plan_one(module, entry, report))
        .count()
}

fn plan_one(module: &mut ModuleBuild, entry: &TextEntry, report: &mut Report) -> bool {
    let backend = module.backend.key();
    let mut who: Vec<&str> = Vec::new();
    for (id, _) in &entry.history {
        if !who.contains(&id.as_str()) {
            who.push(id);
        }
    }
    let label = format!(
        "\"{}\" -> \"{}\" [{}]",
        one_line(&entry.original, 40),
        one_line(&entry.value, 60),
        who.join("+")
    );
    let (Some(old), Some(mut new)) = (latin1(&entry.original), latin1(&entry.value)) else {
        report.warn(format!(
            "{backend}: {label}: only Latin-1 text can be patched; skipped"
        ));
        return false;
    };
    if new.iter().any(|c| !(0x20..=0x7E).contains(c)) {
        report.warn(format!("{backend}: {label}: contains characters outside printable ASCII; the game fonts only have ASCII glyphs"));
    }

    let found = string_sites(module.image, module.refs, &old);
    if found.is_empty() {
        let mut terminated = old.clone();
        terminated.push(0);
        let why = if module
            .image
            .find_bytes(&terminated, &[".rdata", ".data"])
            .is_empty()
        {
            "is not in this build as a string (text the game writes into its code as a constant is not supported yet)"
        } else {
            "exists but nothing references it directly"
        };
        report.warn(format!(
            "{backend}: {label}: original string {why}; skipped"
        ));
        return false;
    }
    if entry.singular.is_some() {
        report.warn(format!("{backend}: {label}: \"singular\" only works for short text the game appends after a number; ignored"));
        if new == old {
            return false;
        }
    }

    let sites = || found.values().flatten();
    let inline = sites().filter(|s| s.kind == SiteKind::Inline).count();
    let patchable = sites().filter(|s| s.kind != SiteKind::Inline).count();
    let sized = sites()
        .filter(|s| s.kind != SiteKind::Inline && s.sized)
        .count();
    if patchable == 0 {
        report.warn(format!("{backend}: {label}: the compiler inlined this string into code; it cannot be repointed; skipped"));
        return false;
    }
    if sized > 0 && new.len() != old.len() {
        match entry.fit {
            Fit::Pad => {
                if new.len() > old.len() {
                    report.warn(format!(
                        "{backend}: {label}: truncated to {} characters (fit=pad)",
                        old.len()
                    ));
                }
                new.resize(old.len(), b' ');
            }
            Fit::Force => {}
            Fit::Strict => {
                report.warn(format!(
                    "{backend}: {label}: {sized} of {patchable} reference(s) appear to hard-code the length {len}; make the replacement exactly {len} characters, or set \"fit\": \"pad\" (pad/truncate) or \"force\"; skipped",
                    len = old.len()
                ));
                return false;
            }
        }
    }
    if inline > 0 {
        report.warn(format!(
            "{backend}: {label}: {inline} inlined use(s) keep the original text"
        ));
    }

    let blob_offset = module.plan.intern(&new);
    let mut ops = Vec::new();
    for (&old_target, group) in &found {
        for site in group {
            match site.kind {
                SiteKind::Lea => ops.push(Op::Rel32 {
                    site: site.rva,
                    old_target,
                    blob_offset,
                }),
                SiteKind::Data => ops.push(Op::Ptr64 {
                    site: site.rva,
                    old_target,
                    blob_offset,
                }),
                SiteKind::Inline => {}
            }
        }
    }
    module.plan.groups.push(Group {
        id: group_id(&old),
        label,
        ops,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Backend;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::plan::{BLOB_PAD, ModulePlan};
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    fn collected(manifests: &[&str]) -> (Vec<TextEntry>, Vec<Mod>, Report) {
        let dir = tempfile::tempdir().unwrap();
        for (n, manifest) in manifests.iter().enumerate() {
            write_mod(dir.path(), &format!("m{n}"), manifest);
        }
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let entries = collect(&mut mods, &order, &mut report);
        (entries, mods, report)
    }

    fn entry(original: &str, value: &str, fit: Fit) -> TextEntry {
        TextEntry {
            original: original.into(),
            value: value.into(),
            singular: None,
            fit,
            history: vec![("m".into(), "set")],
            setters: vec!["m".into()],
        }
    }

    /// Plans `entries` against a tiny image and returns the plan and the warnings.
    fn planned(pe: &TinyPe, entries: &[TextEntry]) -> (ModulePlan, Vec<String>) {
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = ModuleBuild {
            backend: Backend::Avx2,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        };
        let mut report = Report::default();
        plan(&mut module, entries, &mut report);
        (module.plan, report.warnings().map(str::to_owned).collect())
    }

    #[test]
    fn operations_from_several_mods_compose() {
        let (entries, _, report) = collected(&[
            r#"{"schema": 1, "id": "names", "version": "1.0.0", "text": [
                {"match": "BUBBLE GUM", "append": " (COLLIE)"},
                {"match": "COTTON CANDY", "set": "CANDY FLOSS"},
                {"match": "SWITCHING REALM", "set": "HOPPING", "fit": "pad"}]}"#,
            r#"{"schema": 1, "id": "tags", "version": "1.0.0", "priority": 1, "text": [
                {"match": "BUBBLE GUM", "append": " [A]"},
                {"match": "BUBBLE GUM", "prepend": "> "},
                {"match": "COTTON CANDY", "set": "FAIRY FLOSS"},
                {"match": "SWITCHING REALM", "replace": {"from": "PP", "to": "p"}}]}"#,
        ]);
        let by = |original: &str| entries.iter().find(|e| e.original == original).unwrap();
        assert_eq!(by("BUBBLE GUM").value, "> BUBBLE GUM (COLLIE) [A]");
        assert_eq!(
            by("BUBBLE GUM").history,
            [
                ("names".to_owned(), "append"),
                ("tags".to_owned(), "append"),
                ("tags".to_owned(), "prepend")
            ]
        );
        assert_eq!(by("COTTON CANDY").value, "FAIRY FLOSS");
        assert_eq!(
            (
                by("SWITCHING REALM").value.as_str(),
                by("SWITCHING REALM").fit
            ),
            ("HOpING", Fit::Pad)
        );
        assert_eq!(
            report.warnings().collect::<Vec<_>>(),
            ["text conflict on \"COTTON CANDY\": set by names, tags; tags wins"]
        );
    }

    #[test]
    fn malformed_patches_become_warnings_on_the_mod() {
        let (entries, mods, _) =
            collected(&[r#"{"schema": 1, "id": "m", "version": "1.0.0", "text": [
            {"set": "no match"},
            {"match": "A"},
            {"match": "B", "set": "b", "fit": "tight"},
            {"match": "C", "append": "c", "singular": "x"},
            {"match": "D", "replace": {"from": "zz", "to": "y"}},
            {"match": "E", "set": "E"}],
            "textFiles": ["../outside.json"]}"#]);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.original.as_str())
                .collect::<Vec<_>>(),
            ["B", "C"]
        );
        assert_eq!(mods[0].warnings.len(), 6);
    }

    #[test]
    fn text_files_add_patches() {
        let dir = tempfile::tempdir().unwrap();
        let folder = write_mod(
            dir.path(),
            "m",
            r#"{"schema": 1, "id": "m", "version": "1.0.0", "textFiles": ["more.json"]}"#,
        );
        fs::write(folder.join("more.json"), r#"[{"match": "A", "set": "B"}]"#).unwrap();
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let entries = collect(&mut mods, &order, &mut Report::default());
        assert_eq!(
            (entries[0].original.as_str(), entries[0].value.as_str()),
            ("A", "B")
        );
    }

    #[test]
    fn repoints_code_and_table_references() {
        let mut pe = TinyPe::new();
        let name = pe.rdata(b"COTTON CANDY\0");
        let lea = pe.lea(name);
        let pointer = pe.pointer(name);
        let (module, warnings) = planned(&pe, &[entry("COTTON CANDY", "FAIRY FLOSS", Fit::Strict)]);

        assert!(warnings.is_empty());
        let offset = BLOB_PAD as u32;
        assert_eq!(&module.blob[BLOB_PAD..BLOB_PAD + 12], b"FAIRY FLOSS\0");
        assert_eq!(
            module.groups[0].ops,
            [
                Op::Rel32 {
                    site: lea,
                    old_target: name,
                    blob_offset: offset
                },
                Op::Ptr64 {
                    site: pointer,
                    old_target: name,
                    blob_offset: offset
                }
            ]
        );
        assert!(module.groups[0].id.starts_with("text:"));
        assert_eq!(
            module.groups[0].label,
            "\"COTTON CANDY\" -> \"FAIRY FLOSS\" [m]"
        );
    }

    #[test]
    fn a_length_carrying_string_needs_the_same_length_or_a_fit() {
        let mut pe = TinyPe::new();
        let name = pe.rdata(b"OFFLINE\0");
        pe.lea(name);
        pe.code(&[0xBA, 7, 0, 0, 0]); // mov edx, 7

        let (module, warnings) = planned(&pe, &[entry("OFFLINE", "DISCONNECTED", Fit::Strict)]);
        assert!(module.groups.is_empty());
        assert!(warnings[0].contains("hard-code the length 7"));

        let (module, warnings) = planned(&pe, &[entry("OFFLINE", "AWAY", Fit::Pad)]);
        assert!(warnings.is_empty());
        assert_eq!(&module.blob[BLOB_PAD..BLOB_PAD + 8], b"AWAY   \0");

        let (module, warnings) = planned(&pe, &[entry("OFFLINE", "DISCONNECTED", Fit::Pad)]);
        assert!(warnings[0].contains("truncated to 7"));
        assert_eq!(&module.blob[BLOB_PAD..BLOB_PAD + 8], b"DISCONN\0");

        let (module, _) = planned(&pe, &[entry("OFFLINE", "DISCONNECTED", Fit::Force)]);
        assert_eq!(&module.blob[BLOB_PAD..BLOB_PAD + 13], b"DISCONNECTED\0");
        let (module, _) = planned(&pe, &[entry("OFFLINE", "GONE OFF", Fit::Strict)]);
        assert!(module.groups.is_empty());
        let (module, _) = planned(&pe, &[entry("OFFLINE", "AFK NOW", Fit::Strict)]);
        assert_eq!(module.groups.len(), 1);
    }

    #[test]
    fn strings_that_cannot_be_repointed_are_reported() {
        let mut pe = TinyPe::new();
        pe.rdata(b"NOBODY\0");
        let inlined = pe.rdata(b"saves/v1\0");
        pe.rip_relative(&[0x0F, 0x10, 0x05], inlined);
        let entries = [
            entry("NOBODY", "x", Fit::Strict),
            entry("saves/v1", "y", Fit::Strict),
            entry("MISSING", "z", Fit::Strict),
            entry("NOBODY", "\u{4e16}", Fit::Strict),
        ];
        let (module, warnings) = planned(&pe, &entries);
        assert!(module.groups.is_empty());
        assert!(warnings[0].contains("nothing references it directly"));
        assert!(warnings[1].contains("inlined this string"));
        assert!(warnings[2].contains("is not in this build"));
        assert!(warnings[3].contains("only Latin-1"));
    }

    #[test]
    fn the_same_replacement_is_stored_once() {
        let mut pe = TinyPe::new();
        let a = pe.rdata(b"ALPHA\0");
        let b = pe.rdata(b"BETA\0");
        pe.lea(a);
        pe.lea(b);
        let (module, _) = planned(
            &pe,
            &[
                entry("ALPHA", "SAME", Fit::Strict),
                entry("BETA", "SAME", Fit::Strict),
            ],
        );
        assert_eq!(module.groups.len(), 2);
        assert_eq!(module.blob.len(), BLOB_PAD + 4 + BLOB_PAD);
    }
}
