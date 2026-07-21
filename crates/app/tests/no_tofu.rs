//! Guard against "tofu": string literals containing a codepoint the egui font
//! stack can't render (shown as an empty box) — fix by swapping in an
//! `egui_phosphor` icon.

use ab_glyph::{Font, FontRef};
use std::path::{Path, PathBuf};

#[test]
fn no_tofu_in_displayed_strings() {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    let names = &fonts.families[&egui::FontFamily::Proportional];
    let datas: Vec<Vec<u8>> = names
        .iter()
        .map(|n| fonts.font_data[n].font.to_vec())
        .collect();
    let faces: Vec<FontRef> = datas
        .iter()
        .filter_map(|b| FontRef::try_from_slice(b).ok())
        .collect();
    let covered = |c: char| c.is_ascii() || faces.iter().any(|f| f.glyph_id(c).0 != 0);

    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders: Vec<String> = Vec::new();
    scan(&src, &mut |path, text| {
        for (i, line) in text.lines().enumerate() {
            for ch in string_literal_chars(line) {
                if !covered(ch) {
                    offenders.push(format!(
                        "{}:{}  U+{:04X} {:?}  {}",
                        path.display(),
                        i + 1,
                        ch as u32,
                        ch,
                        line.trim()
                    ));
                }
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "unrenderable glyph(s) in displayed strings (would show as tofu boxes); \
         replace each with an egui_phosphor icon:\n{}",
        offenders.join("\n")
    );
}

/// Characters that sit inside a `"..."` literal on this line, ignoring anything
/// after a `//` line comment. Deliberately simple: single-line strings only,
/// which is all the codebase's user-facing labels use.
fn string_literal_chars(line: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut in_str = false;
    let mut prev = '\0';
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if !in_str && ch == '/' && chars.peek() == Some(&'/') {
            break;
        }
        if ch == '"' && prev != '\\' {
            in_str = !in_str;
            prev = ch;
            continue;
        }
        if in_str {
            out.push(ch);
        }
        prev = ch;
    }
    out
}

fn scan(dir: &Path, f: &mut impl FnMut(&Path, &str)) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, f);
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            f(&path, &text);
        }
    }
}
