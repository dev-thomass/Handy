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
use serde::Serialize;
use specta::Type;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// A pasted chunk and the corrected chunk must be at least this similar to
/// count as a misheard word rather than a rewrite. Mishearings of names and
/// jargon are often far from the right spelling ("Clode" → "Claude"), so this
/// stays permissive; the word-count limits below keep rewrites out.
const MIN_SIMILARITY: f64 = 0.4;
/// Longest chunk (in words) treated as a correction.
const MAX_CHUNK_WORDS: usize = 4;
const MAX_CHUNK_CHARS: usize = 60;
/// Longer replaced chunks are split into word-by-word corrections.
const PHRASE_WORDS: usize = 2;

/// Bumped on every paste so an older watcher stops when a new dictation lands.
static WATCH_GENERATION: AtomicU64 = AtomicU64::new(0);

// ---- Diagnostic log -------------------------------------------------------

const LOG_CAPACITY: usize = 60;

/// One line of the auto-learn journal shown in the settings, so a user can
/// see what was learned and, more importantly, why an edit was not.
#[derive(Clone, Debug, Serialize, Type)]
pub struct AutoLearnLogEntry {
    /// Unix time in milliseconds.
    pub at: f64,
    /// Name of the app the dictation went to, when known.
    pub app: Option<String>,
    /// i18n key under `settings.learning.autoLearnLog.codes.`
    pub code: String,
    pub misheard: Option<String>,
    pub word: Option<String>,
}

static LOG: Mutex<VecDeque<AutoLearnLogEntry>> = Mutex::new(VecDeque::new());

pub fn log_event(code: &str, app: Option<String>, misheard: Option<&str>, word: Option<&str>) {
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or_default();
    log::info!("auto-learn: {} (app: {:?})", code, app);
    if let Ok(mut log) = LOG.lock() {
        if log.len() >= LOG_CAPACITY {
            log.pop_front();
        }
        log.push_back(AutoLearnLogEntry {
            at,
            app,
            code: code.to_string(),
            misheard: misheard.map(str::to_string),
            word: word.map(str::to_string),
        });
    }
}

/// Newest first.
pub fn log_entries() -> Vec<AutoLearnLogEntry> {
    LOG.lock()
        .map(|log| log.iter().rev().cloned().collect())
        .unwrap_or_default()
}

pub fn clear_log() {
    if let Ok(mut log) = LOG.lock() {
        log.clear();
    }
}

/// Name of the frontmost app, for the journal.
pub fn focused_app_name() -> Option<String> {
    platform::focused_app_name()
}

// ---- Diffing --------------------------------------------------------------

fn tokens(text: &str) -> Vec<&str> {
    text.split_whitespace().collect()
}

fn clean(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
}

/// Text fields hand back what was pasted with small differences: non-breaking
/// spaces, curly apostrophes, zero-width characters, `\r\n`, collapsed or
/// doubled spaces. Compare everything in this canonical form.
pub fn normalize(text: &str) -> String {
    let mapped: String = text
        .chars()
        .filter(|c| !matches!(c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}'))
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{02bc}' => '\'',
            c if c.is_whitespace() => ' ',
            c => c,
        })
        .collect();
    mapped
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Share of the dictated words still present in `value`. Lets the watcher
/// tell a corrected field from one that was sent, cleared or replaced.
pub fn dictation_overlap(pasted: &str, value: &str) -> f64 {
    let pasted: Vec<String> = tokens(pasted)
        .into_iter()
        .map(|w| clean(w).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    if pasted.is_empty() {
        return 0.0;
    }
    let present: std::collections::HashSet<String> = tokens(value)
        .into_iter()
        .map(|w| clean(w).to_lowercase())
        .collect();
    pasted.iter().filter(|w| present.contains(*w)).count() as f64 / pasted.len() as f64
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

/// What became of one edit the user made to the dictated text.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Learn,
    /// Too many words or characters to be a misheard word.
    TooLong,
    /// The edited words were typed by the user, not dictated.
    NotDictated,
    /// The new words don't look like the old ones: a rewrite, not a fix.
    TooDifferent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub misheard: String,
    pub word: String,
    pub verdict: Verdict,
}

/// Every replacement the user made to `pasted` between two readings of the
/// text field (`before` right after the paste, `after` once they are done),
/// with whether it can be learned. Inputs are expected in [`normalize`]d form.
pub fn analyze_edits(pasted: &str, before: &str, after: &str) -> Vec<Edit> {
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

    let judge = |old: &[&str], new: &[&str]| -> Option<Edit> {
        let misheard = chunk_text(old);
        let word = chunk_text(new);
        if misheard.is_empty() || word.is_empty() || misheard == word {
            return None;
        }
        let verdict = if old.len() > MAX_CHUNK_WORDS
            || new.len() > MAX_CHUNK_WORDS
            || word.chars().count() > MAX_CHUNK_CHARS
        {
            Verdict::TooLong
        } else if whole_field
            && !pasted_words
                .windows(old.len())
                .any(|w| w.iter().zip(old).all(|(p, o)| *p == clean(o)))
        {
            Verdict::NotDictated
        } else if similarity(&misheard, &word) < MIN_SIMILARITY {
            Verdict::TooDifferent
        } else {
            Verdict::Learn
        };
        Some(Edit {
            misheard,
            word,
            verdict,
        })
    };

    replaced_chunks(&old_words, &new_words)
        .into_iter()
        .flat_map(|(old, new)| {
            let Some(whole) = judge(&old, &new) else {
                return Vec::new();
            };
            // Half a sentence fixed in one go is one long replaced chunk:
            // pair its words up and learn each misheard word on its own. Two
            // words ("cloud code" → "Claude Code") stay one phrase.
            let long = old.len() > PHRASE_WORDS || new.len() > PHRASE_WORDS;
            if (long || whole.verdict != Verdict::Learn) && (old.len() > 1 || new.len() > 1) {
                let parts: Vec<Edit> = pair_words(&old, &new)
                    .into_iter()
                    .filter_map(|(o, n)| judge(&o, &n))
                    .collect();
                if parts.iter().any(|e| e.verdict == Verdict::Learn) {
                    return parts;
                }
            }
            vec![whole]
        })
        .collect()
}

fn similarity(a: &str, b: &str) -> f64 {
    strsim::normalized_levenshtein(
        &a.to_lowercase().replace(' ', ""),
        &b.to_lowercase().replace(' ', ""),
    )
}

/// Align the words of a long replaced chunk with the words that replaced
/// them, pairing each misheard word with the one that looks most like it.
/// A word may split in two ("chatgpt" → "chat GPT") or two may merge; words
/// with no look-alike are left out. Returns the pairs in order.
fn pair_words<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Vec<&'a str>, Vec<&'a str>)> {
    let (n, m) = (old.len(), new.len());
    // Short words ("le" → "la") look alike by chance: only pair them when
    // the fix is about case.
    let score = |o: &[&str], w: &[&str]| -> Option<f64> {
        let (a, b) = (chunk_text(o), chunk_text(w));
        let short = a.chars().count() < 3 || b.chars().count() < 3;
        if short && a.to_lowercase() != b.to_lowercase() {
            return None;
        }
        let sim = similarity(&a, &b);
        (sim >= MIN_SIMILARITY).then_some(sim)
    };
    const SHAPES: [(usize, usize); 3] = [(1, 1), (1, 2), (2, 1)];
    let mut best = vec![vec![0.0f64; m + 1]; n + 1];
    let mut step = vec![vec![(0usize, 0usize, false); m + 1]; n + 1];
    for i in 0..=n {
        for j in 0..=m {
            if i == 0 && j == 0 {
                continue;
            }
            let mut choice = (0.0, (0, 0, false));
            if i > 0 && best[i - 1][j] >= choice.0 {
                choice = (best[i - 1][j], (1, 0, false));
            }
            if j > 0 && best[i][j - 1] >= choice.0 {
                choice = (best[i][j - 1], (0, 1, false));
            }
            for (a, b) in SHAPES {
                if i >= a && j >= b {
                    if let Some(sim) = score(&old[i - a..i], &new[j - b..j]) {
                        let total = best[i - a][j - b] + sim;
                        if total > choice.0 {
                            choice = (total, (a, b, true));
                        }
                    }
                }
            }
            best[i][j] = choice.0;
            step[i][j] = choice.1;
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        let (a, b, paired) = step[i][j];
        if a == 0 && b == 0 {
            break;
        }
        if paired {
            pairs.push((old[i - a..i].to_vec(), new[j - b..j].to_vec()));
        }
        i -= a;
        j -= b;
    }
    pairs.reverse();
    pairs
}

/// The corrections worth learning among the user's edits.
pub fn find_corrections(pasted: &str, before: &str, after: &str) -> Vec<LearnedCorrection> {
    analyze_edits(pasted, before, after)
        .into_iter()
        .filter(|e| e.verdict == Verdict::Learn)
        .map(|e| LearnedCorrection {
            word: e.word,
            misheard: Some(e.misheard),
        })
        .collect()
}

/// How watching the field after a paste ended.
#[derive(Clone, Debug, PartialEq)]
pub enum WatchOutcome {
    /// The field's text can't be read through Accessibility in this app.
    Unreadable,
    /// The field was readable but the dictation never showed up in it.
    PasteNotFound,
    /// The user didn't touch the dictated text.
    NoEdit,
    /// Normalized field text right after the paste and once the user was done.
    Edited { before: String, after: String },
}

/// Start watching the focused text field for corrections to `pasted`.
/// Logs what happened and runs `on_learned` with whatever can be learned
/// (never with an empty list).
pub fn watch_after_paste<F>(pasted: String, on_learned: F)
where
    F: FnOnce(Vec<LearnedCorrection>, Option<String>) + Send + 'static,
{
    let generation = WATCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        let still_current = || WATCH_GENERATION.load(Ordering::SeqCst) == generation;
        let app = focused_app_name();
        let pasted = normalize(&pasted);
        let outcome = platform::watch(&pasted, &still_current);
        let (before, after) = match outcome {
            WatchOutcome::Unreadable => return log_event("unreadable", app, None, None),
            WatchOutcome::PasteNotFound => return log_event("pasteNotFound", app, None, None),
            WatchOutcome::NoEdit => return log_event("noEdit", app, None, None),
            WatchOutcome::Edited { before, after } => (before, after),
        };
        let edits = analyze_edits(&pasted, &before, &after);
        if edits.is_empty() {
            return log_event("noWordEdit", app, None, None);
        }
        let mut learned = Vec::new();
        for edit in edits {
            let code = match edit.verdict {
                Verdict::Learn => {
                    learned.push(LearnedCorrection {
                        word: edit.word,
                        misheard: Some(edit.misheard),
                    });
                    continue;
                }
                Verdict::TooLong => "tooLong",
                Verdict::NotDictated => "notDictated",
                Verdict::TooDifferent => "tooDifferent",
            };
            log_event(code, app.clone(), Some(&edit.misheard), Some(&edit.word));
        }
        if !learned.is_empty() {
            on_learned(learned, app);
        }
    });
}

/// What the focused UI element is, as far as pasting goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    /// An editable text field: paste.
    Text,
    /// Something focused, but nothing that takes text (desktop, a web page,
    /// a list…): pasting would go nowhere.
    NoText,
    /// The Accessibility permission is missing (or went stale after an
    /// update), so a simulated ⌘V would be silently dropped.
    NoAccess,
    /// Can't tell (not macOS, or the app doesn't answer): paste as usual.
    Unknown,
}

pub fn focus_target() -> FocusTarget {
    platform::focus_target()
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{dictation_overlap, normalize, FocusTarget, WatchOutcome};
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type AXUIElementRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXUIElementIsAttributeSettable(
            element: AXUIElementRef,
            attribute: CFStringRef,
            settable: *mut u8,
        ) -> i32;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: CFTypeRef;
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

    fn copy_attribute_raw(element: CFTypeRef, name: &str) -> Result<Cf, i32> {
        let attribute = cf_string(name);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(element, attribute.0, &mut value) };
        if err == 0 && !value.is_null() {
            Ok(Cf(value))
        } else {
            Err(err)
        }
    }

    fn copy_attribute(element: CFTypeRef, name: &str) -> Option<Cf> {
        copy_attribute_raw(element, name).ok()
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

    fn system() -> Cf {
        Cf(unsafe { AXUIElementCreateSystemWide() })
    }

    fn focused_element() -> Option<Cf> {
        copy_attribute(system().0, "AXFocusedUIElement")
    }

    fn focused_app() -> Option<Cf> {
        copy_attribute(system().0, "AXFocusedApplication")
    }

    pub fn focused_app_name() -> Option<String> {
        to_string(&copy_attribute(focused_app()?.0, "AXTitle")?)
    }

    fn set_true(element: CFTypeRef, name: &str) -> bool {
        let attribute = cf_string(name);
        unsafe { AXUIElementSetAttributeValue(element, attribute.0, kCFBooleanTrue) == 0 }
    }

    /// Chromium browsers and Electron apps (Slack, Notion, Discord, VS Code…)
    /// only build their accessibility tree when they think a screen reader is
    /// running. Until then their text fields are invisible: the focused
    /// element is the window, and its text can't be read. Ask for the tree.
    /// Returns whether the app accepted either request.
    fn wake_app_accessibility() -> bool {
        let Some(app) = focused_app() else {
            return false;
        };
        let electron = set_true(app.0, "AXManualAccessibility");
        let chromium = set_true(app.0, "AXEnhancedUserInterface");
        electron || chromium
    }

    const TEXT_ROLES: [&str; 4] = ["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];
    const AX_ERROR_API_DISABLED: i32 = -25211;

    fn is_settable(element: CFTypeRef, name: &str) -> bool {
        let attribute = cf_string(name);
        let mut settable: u8 = 0;
        let err = unsafe { AXUIElementIsAttributeSettable(element, attribute.0, &mut settable) };
        err == 0 && settable != 0
    }

    fn classify() -> FocusTarget {
        let element = match copy_attribute_raw(system().0, "AXFocusedUIElement") {
            Ok(element) => element,
            Err(AX_ERROR_API_DISABLED) => return FocusTarget::NoAccess,
            // No focused element at all: the desktop, or an app with no
            // window. Pasting there does nothing.
            Err(_) => return FocusTarget::NoText,
        };
        let role = copy_attribute(element.0, "AXRole").and_then(|r| to_string(&r));
        if role.as_deref().is_some_and(|r| TEXT_ROLES.contains(&r)) {
            return FocusTarget::Text;
        }
        // Rich editors (contenteditable in browsers, Slack, Notion…) either
        // have a writable value or sit inside an editable ancestor. A plain
        // web page, window or list only exposes a selection range, which is
        // not enough to take a paste.
        if is_settable(element.0, "AXValue")
            || copy_attribute(element.0, "AXEditableAncestor").is_some()
        {
            return FocusTarget::Text;
        }
        FocusTarget::NoText
    }

    pub fn focus_target() -> FocusTarget {
        if unsafe { AXIsProcessTrusted() } == 0 {
            return FocusTarget::NoAccess;
        }
        match classify() {
            // Maybe a browser or Electron app that hasn't built its tree yet.
            FocusTarget::NoText if wake_app_accessibility() => {
                std::thread::sleep(Duration::from_millis(150));
                classify()
            }
            target => target,
        }
    }

    fn read_value(element: &Cf) -> Option<String> {
        to_string(&copy_attribute(element.0, "AXValue")?).map(|v| normalize(&v))
    }

    /// How long to wait for the paste to show up in the field.
    const PASTE_LANDS_WITHIN: Duration = Duration::from_millis(2500);
    const POLL: Duration = Duration::from_millis(500);
    /// Stop once the text has not changed for this long after an edit.
    const SETTLE: Duration = Duration::from_secs(10);
    /// Give up if nothing at all is edited for this long.
    const IDLE: Duration = Duration::from_secs(45);
    const MAX_WATCH: Duration = Duration::from_secs(120);
    /// A field still holding this share of the dictated words is the same
    /// text being corrected, not a sent or cleared message.
    const SAME_TEXT: f64 = 0.5;

    pub fn watch(pasted: &str, still_current: &dyn Fn() -> bool) -> WatchOutcome {
        // Wait for the target app to apply the paste.
        let waiting = Instant::now();
        let mut readable = false;
        let mut woken = false;
        let mut found = None;
        while waiting.elapsed() < PASTE_LANDS_WITHIN && still_current() {
            std::thread::sleep(Duration::from_millis(250));
            let Some(value_and_element) =
                focused_element().and_then(|el| read_value(&el).map(|v| (el, v)))
            else {
                if !woken {
                    woken = true;
                    wake_app_accessibility();
                }
                continue;
            };
            let (element, value) = value_and_element;
            readable = true;
            if value.contains(pasted) {
                found = Some((element, value));
                break;
            }
        }
        let Some((mut element, before)) = found else {
            return if readable {
                WatchOutcome::PasteNotFound
            } else {
                WatchOutcome::Unreadable
            };
        };

        let started = Instant::now();
        let mut latest = before.clone();
        // Last text that still held the dictation. When the user fixes a word
        // then presses Enter, the field empties: learn from what was sent.
        let mut best = before.clone();
        let mut last_change: Option<Instant> = None;
        while started.elapsed() < MAX_WATCH && still_current() {
            std::thread::sleep(POLL);
            let Some(now) = focused_element() else {
                break;
            };
            let value = if unsafe { CFEqual(now.0, element.0) } != 0 {
                read_value(&element)
            } else {
                // Web editors often swap the focused node while typing. Follow
                // the focus if the new element still holds the dictation;
                // otherwise the user moved on.
                match read_value(&now) {
                    Some(value) if dictation_overlap(pasted, &value) >= SAME_TEXT => {
                        element = now;
                        Some(value)
                    }
                    _ => break,
                }
            };
            let Some(value) = value else {
                break;
            };
            if value != latest {
                latest = value;
                last_change = Some(Instant::now());
                if dictation_overlap(pasted, &latest) >= SAME_TEXT {
                    best = latest.clone();
                } else {
                    // Sent, cleared or replaced: the edit is over.
                    break;
                }
            } else {
                match last_change {
                    Some(t) if t.elapsed() >= SETTLE => break,
                    None if started.elapsed() >= IDLE => break,
                    _ => {}
                }
            }
        }
        if best == before {
            WatchOutcome::NoEdit
        } else {
            WatchOutcome::Edited {
                before,
                after: best,
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::{FocusTarget, WatchOutcome};

    pub fn focus_target() -> FocusTarget {
        FocusTarget::Unknown
    }

    pub fn focused_app_name() -> Option<String> {
        None
    }

    pub fn watch(_pasted: &str, _still_current: &dyn Fn() -> bool) -> WatchOutcome {
        WatchOutcome::Unreadable
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

    #[test]
    fn normalizes_what_fields_hand_back() {
        assert_eq!(
            normalize("l\u{2019}IA\u{a0}de  Claude\r\n\u{200b}ok"),
            "l'IA de Claude ok"
        );
        let pasted = normalize("j'aime l'IA");
        let field = normalize("Note : j\u{2019}aime l\u{2019}IA\u{a0}");
        assert!(field.contains(&pasted));
    }

    #[test]
    fn learns_distant_mishearings_of_names() {
        let pasted = "demande à Clode de relire";
        assert_eq!(
            pairs(find_corrections(
                pasted,
                pasted,
                "demande à Claude de relire"
            )),
            vec![("Clode".into(), "Claude".into())]
        );
    }

    #[test]
    fn explains_why_edits_are_not_learned() {
        let pasted = "On se voit demain matin";
        let edits = analyze_edits(pasted, pasted, "On se voit jeudi soir");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].verdict, Verdict::TooDifferent);
    }

    #[test]
    fn tells_a_corrected_field_from_a_sent_one() {
        let pasted = "Salut Tomas on se voit demain";
        assert!(dictation_overlap(pasted, "Salut Thomas on se voit demain") >= 0.5);
        assert!(dictation_overlap(pasted, "") < 0.5);
        assert!(dictation_overlap(pasted, "nouveau message") < 0.5);
    }

    #[test]
    fn learns_every_word_of_a_rewritten_half_sentence() {
        let pasted = "je pense que le projet Wisper flo et cloud code sont top";
        let after = "je pense que le projet Wispr Flow et Claude Code sont top";
        assert_eq!(
            pairs(find_corrections(pasted, pasted, after)),
            vec![
                ("Wisper flo".into(), "Wispr Flow".into()),
                ("cloud code".into(), "Claude Code".into()),
            ]
        );
        // A longer run with no shared word in between is split word by word.
        let pasted = "rendez-vous avec Tomas Bonardelle a Marseil demain";
        let after = "rendez-vous avec Thomas Bonnardel à Marseille demain";
        let found = pairs(find_corrections(pasted, pasted, after));
        assert!(found.contains(&("Tomas".into(), "Thomas".into())));
        assert!(found.contains(&("Bonardelle".into(), "Bonnardel".into())));
        assert!(found.contains(&("Marseil".into(), "Marseille".into())));
    }

    #[test]
    fn pairs_words_that_split_or_merge() {
        let pasted = "on utilise chatgpt et la base super base aujourd'hui";
        let after = "on utilise chat GPT et la base Supabase aujourd'hui";
        let found = pairs(find_corrections(pasted, pasted, after));
        assert!(found.contains(&("chatgpt".into(), "chat GPT".into())));
        assert!(found.contains(&("super base".into(), "Supabase".into())));
    }
}
