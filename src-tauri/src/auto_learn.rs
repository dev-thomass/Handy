//! Learn from corrections the user makes right after a dictation is pasted.
//!
//! After a paste, the focused text field is read through the macOS
//! Accessibility API and watched for a short while. When the user retypes a
//! word of the pasted text ("Tomas" → "Thomas"), the edit is folded into the
//! vocabulary exactly like the manual "learn correction" shortcut does, so the
//! next dictation gets it right. Only word-sized replacements that look like
//! the original are learned; rewrites, additions and deletions are ignored.

// Only the macOS watcher drives the diffing; elsewhere it is test-only.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::learning::LearnedCorrection;
use std::sync::atomic::{AtomicU64, Ordering};

/// A pasted chunk and the corrected chunk must be at least this similar to
/// count as a misheard word rather than a rewrite.
const MIN_SIMILARITY: f64 = 0.5;
/// Longest chunk (in words) treated as a correction.
const MAX_CHUNK_WORDS: usize = 4;
const MAX_CHUNK_CHARS: usize = 60;

/// Bumped on every paste so an older watcher stops when a new dictation lands.
static WATCH_GENERATION: AtomicU64 = AtomicU64::new(0);

fn tokens(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

fn clean(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
}

/// Word-level alignment of `old` and `new` (longest common subsequence on the
/// cleaned, case-sensitive words). Returns the replaced chunks as
/// `(old words, new words)` pairs; pure insertions and deletions are dropped.
fn replaced_chunks<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Vec<&'a str>, Vec<&'a str>)> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if clean(old[i]) == clean(new[j]) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut chunks = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (mut old_run, mut new_run): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
    let mut flush = |old_run: &mut Vec<&'a str>, new_run: &mut Vec<&'a str>| {
        if !old_run.is_empty() && !new_run.is_empty() {
            chunks.push((std::mem::take(old_run), std::mem::take(new_run)));
        } else {
            old_run.clear();
            new_run.clear();
        }
    };
    while i < n || j < m {
        if i < n && j < m && clean(old[i]) == clean(new[j]) {
            flush(&mut old_run, &mut new_run);
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            new_run.push(new[j]);
            j += 1;
        } else {
            old_run.push(old[i]);
            i += 1;
        }
    }
    flush(&mut old_run, &mut new_run);
    chunks
}

fn chunk_text(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| clean(w))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Find the corrections the user made to `pasted` between two readings of the
/// text field (`before` right after the paste, `after` once they are done).
pub fn find_corrections(pasted: &str, before: &str, after: &str) -> Vec<LearnedCorrection> {
    let pasted = pasted.trim();
    if pasted.is_empty() || before == after {
        return Vec::new();
    }
    // Only look at edits inside the dictated text, not elsewhere in the field.
    let Some(start) = before.rfind(pasted) else {
        return Vec::new();
    };
    let prefix = &before[..start];
    let suffix = &before[start + pasted.len()..];
    let (old_text, new_text, whole_field) = match after
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
    {
        Some(region) => (pasted, region, false),
        // The user also edited around the dictation: compare the whole field
        // and keep only the fixes to words that came from the dictation.
        None => (before, after, true),
    };
    let pasted_words: Vec<&str> = tokens(pasted).into_iter().map(clean).collect();
    let old_words = tokens(old_text);
    let new_words = tokens(new_text);

    replaced_chunks(&old_words, &new_words)
        .into_iter()
        .filter_map(|(old, new)| {
            if old.len() > MAX_CHUNK_WORDS || new.len() > MAX_CHUNK_WORDS {
                return None;
            }
            if whole_field
                && !pasted_words
                    .windows(old.len())
                    .any(|w| w.iter().zip(&old).all(|(p, o)| *p == clean(o)))
            {
                return None;
            }
            let misheard = chunk_text(&old);
            let word = chunk_text(&new);
            if misheard.is_empty()
                || word.is_empty()
                || misheard == word
                || word.chars().count() > MAX_CHUNK_CHARS
            {
                return None;
            }
            let similarity = strsim::normalized_levenshtein(
                &misheard.to_lowercase().replace(' ', ""),
                &word.to_lowercase().replace(' ', ""),
            );
            (similarity >= MIN_SIMILARITY).then_some(LearnedCorrection {
                word,
                misheard: Some(misheard),
            })
        })
        .collect()
}

/// Start watching the focused text field for corrections to `pasted`.
/// `on_learned` runs once with whatever was learned (never with an empty list).
pub fn watch_after_paste<F>(pasted: String, on_learned: F)
where
    F: FnOnce(Vec<LearnedCorrection>) + Send + 'static,
{
    let generation = WATCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let still_current = || WATCH_GENERATION.load(Ordering::SeqCst) == generation;
        if let Some(corrections) = platform::watch(&pasted, &still_current) {
            if !corrections.is_empty() {
                on_learned(corrections);
            }
        }
    });
}

/// Whether the focused UI element can take text. `None` when unknown
/// (not macOS, or Accessibility unavailable): callers should paste as usual.
pub fn focus_accepts_text() -> Option<bool> {
    platform::focus_accepts_text()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::find_corrections;
    use crate::learning::LearnedCorrection;
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type AXUIElementRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithBytes(
            alloc: CFTypeRef,
            bytes: *const u8,
            len: isize,
            encoding: u32,
            external: u8,
        ) -> CFStringRef;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetCharacters(s: CFStringRef, range: CFRange, buffer: *mut u16);
        fn CFStringGetTypeID() -> usize;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
        fn CFRelease(cf: CFTypeRef);
    }

    #[repr(C)]
    struct CFRange {
        location: isize,
        length: isize,
    }

    const UTF8: u32 = 0x0800_0100;

    /// Owned CoreFoundation reference, released on drop.
    struct Cf(CFTypeRef);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn cf_string(s: &str) -> Cf {
        Cf(unsafe {
            CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0)
        })
    }

    fn copy_attribute(element: CFTypeRef, name: &str) -> Option<Cf> {
        let attribute = cf_string(name);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(element, attribute.0, &mut value) };
        (err == 0 && !value.is_null()).then(|| Cf(value))
    }

    fn to_string(value: &Cf) -> Option<String> {
        unsafe {
            if CFGetTypeID(value.0) != CFStringGetTypeID() {
                return None;
            }
            let len = CFStringGetLength(value.0);
            let mut buf = vec![0u16; len.max(0) as usize];
            CFStringGetCharacters(
                value.0,
                CFRange {
                    location: 0,
                    length: len,
                },
                buf.as_mut_ptr(),
            );
            Some(String::from_utf16_lossy(&buf))
        }
    }

    fn focused_element() -> Option<Cf> {
        let system = Cf(unsafe { AXUIElementCreateSystemWide() });
        copy_attribute(system.0, "AXFocusedUIElement")
    }

    const TEXT_ROLES: [&str; 4] = ["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];

    pub fn focus_accepts_text() -> Option<bool> {
        // Without Accessibility nothing can be read: stay out of the way.
        let system = Cf(unsafe { AXUIElementCreateSystemWide() });
        let mut probe: CFTypeRef = std::ptr::null();
        let attribute = cf_string("AXFocusedApplication");
        let err = unsafe { AXUIElementCopyAttributeValue(system.0, attribute.0, &mut probe) };
        if err != 0 || probe.is_null() {
            return None;
        }
        drop(Cf(probe));

        let Some(element) = focused_element() else {
            return Some(false);
        };
        let role = copy_attribute(element.0, "AXRole").and_then(|r| to_string(&r));
        if role.as_deref().is_some_and(|r| TEXT_ROLES.contains(&r)) {
            return Some(true);
        }
        // Rich editors (web contenteditable, Slack, Notion…) expose a text
        // selection even when their role is generic.
        Some(copy_attribute(element.0, "AXSelectedTextRange").is_some())
    }

    fn read_value(element: &Cf) -> Option<String> {
        to_string(&copy_attribute(element.0, "AXValue")?)
    }

    const POLL: Duration = Duration::from_millis(700);
    /// Stop once the text has not changed for this long after an edit.
    const SETTLE: Duration = Duration::from_secs(5);
    const MAX_WATCH: Duration = Duration::from_secs(90);

    pub fn watch(pasted: &str, still_current: &dyn Fn() -> bool) -> Option<Vec<LearnedCorrection>> {
        // Give the target app time to apply the paste.
        std::thread::sleep(Duration::from_millis(400));
        let element = focused_element()?;
        let before = read_value(&element)?;
        if !before.contains(pasted.trim()) {
            // Secure field, unsupported app, or the paste went elsewhere.
            return None;
        }

        let started = Instant::now();
        let mut latest = before.clone();
        let mut last_change: Option<Instant> = None;
        while started.elapsed() < MAX_WATCH && still_current() {
            std::thread::sleep(POLL);
            // The user moved to another field or app: they are done here.
            let same_field = focused_element()
                .map(|now| unsafe { CFEqual(now.0, element.0) } != 0)
                .unwrap_or(false);
            if !same_field {
                break;
            }
            match read_value(&element) {
                Some(value) if value != latest => {
                    latest = value;
                    last_change = Some(Instant::now());
                }
                Some(_) => {
                    if last_change.is_some_and(|t| t.elapsed() >= SETTLE) {
                        break;
                    }
                }
                None => break,
            }
        }
        if !still_current() && last_change.is_none() {
            return None;
        }
        Some(find_corrections(pasted, &before, &latest))
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use crate::learning::LearnedCorrection;

    pub fn focus_accepts_text() -> Option<bool> {
        None
    }

    pub fn watch(
        _pasted: &str,
        _still_current: &dyn Fn() -> bool,
    ) -> Option<Vec<LearnedCorrection>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(found: Vec<LearnedCorrection>) -> Vec<(String, String)> {
        found
            .into_iter()
            .map(|c| (c.misheard.unwrap_or_default(), c.word))
            .collect()
    }

    #[test]
    fn learns_a_retyped_name() {
        let pasted = "Salut Tomas, on se voit demain ?";
        let before = format!("Objet : test\n{}", pasted);
        let after = "Objet : test\nSalut Thomas, on se voit demain ?";
        assert_eq!(
            pairs(find_corrections(pasted, &before, after)),
            vec![("Tomas".into(), "Thomas".into())]
        );
    }

    #[test]
    fn learns_split_and_case_fixes() {
        let pasted = "j'utilise chat GPT et github";
        let after = "j'utilise ChatGPT et GitHub";
        assert_eq!(
            pairs(find_corrections(pasted, pasted, after)),
            vec![
                ("chat GPT".into(), "ChatGPT".into()),
                ("github".into(), "GitHub".into())
            ]
        );
    }

    #[test]
    fn ignores_rewrites_additions_and_punctuation() {
        let pasted = "On se voit demain matin";
        // Different words, not a mishearing.
        assert!(find_corrections(pasted, pasted, "On se voit jeudi soir").is_empty());
        // Words added at the end.
        assert!(find_corrections(pasted, pasted, "On se voit demain matin vers 9h").is_empty());
        // Punctuation only.
        assert!(find_corrections(pasted, pasted, "On se voit demain matin.").is_empty());
    }

    #[test]
    fn ignores_edits_when_paste_not_found() {
        assert!(find_corrections("Tomas", "autre chose", "autre chose Thomas").is_empty());
    }

    #[test]
    fn ignores_edits_outside_the_pasted_text() {
        let pasted = "Salut Thomas";
        let before = format!("Bonjur\n{}", pasted);
        let after = "Bonjour\nSalut Thomas";
        // The fix is in text the user typed, not in the dictation.
        assert!(find_corrections(pasted, &before, after)
            .iter()
            .all(|c| c.misheard.as_deref() != Some("Bonjur")));
    }
}
