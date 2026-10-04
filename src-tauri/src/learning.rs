//! Personal learning: the pieces that make dictation adapt to the user.
//!
//! - **Corrections**: after a dictation, the user selects the word they fixed
//!   and presses the "learn correction" shortcut. The selection is compared
//!   with the last dictated text to find what was misheard; the correct
//!   spelling joins the custom words and a `misheard → correct` replacement is
//!   recorded so the same mistake is fixed automatically next time.
//! - **Text replacements**: user-defined or learned `from → to` pairs applied
//!   to every transcription (also used as voice snippets: "my address" →
//!   the full address).
//! - **Per-app prompts**: the post-processing prompt is chosen from the app
//!   that was focused when recording started.

use crate::settings::{AppPromptRule, TextReplacement};
use log::debug;
use once_cell::sync::Lazy;
use regex::{Regex, RegexBuilder};
use std::sync::Mutex;

/// Longest selection (in words) that is treated as a correction.
const MAX_CORRECTION_WORDS: usize = 4;
/// Longest selection (in characters) that is treated as a correction.
const MAX_CORRECTION_CHARS: usize = 60;
/// Minimum similarity between the selection and a dictated n-gram for the
/// n-gram to be considered the misheard version of the selection.
const MIN_MISHEARD_SIMILARITY: f64 = 0.3;

/// The application that had focus when the current recording started.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ActiveApp {
    /// Process / application name, e.g. "Slack", "chrome", "Code".
    pub name: String,
    /// Window title, when the platform exposes it (empty otherwise).
    pub title: String,
}

impl ActiveApp {
    /// Human-readable label used for the `${app}` prompt placeholder.
    pub fn label(&self) -> String {
        match (self.name.is_empty(), self.title.is_empty()) {
            (false, false) => format!("{} — {}", self.name, self.title),
            (false, true) => self.name.clone(),
            (true, false) => self.title.clone(),
            (true, true) => String::new(),
        }
    }
}

static RECORDING_APP: Lazy<Mutex<Option<ActiveApp>>> = Lazy::new(|| Mutex::new(None));
static LAST_OUTPUT: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

/// Remember which app was focused when recording started.
pub fn capture_recording_app() {
    let app = active_app();
    debug!("Active app at recording start: {:?}", app);
    if let Ok(mut slot) = RECORDING_APP.lock() {
        *slot = app;
    }
}

/// The app captured by [`capture_recording_app`] for the current recording.
pub fn recording_app() -> Option<ActiveApp> {
    RECORDING_APP.lock().ok().and_then(|slot| slot.clone())
}

/// Remember the text that was just inserted, so a correction can be compared
/// against it.
pub fn set_last_output(text: &str) {
    if let Ok(mut slot) = LAST_OUTPUT.lock() {
        *slot = Some(text.to_string());
    }
}

pub fn last_output() -> Option<String> {
    LAST_OUTPUT.lock().ok().and_then(|slot| slot.clone())
}

// ---------------------------------------------------------------------------
// Corrections
// ---------------------------------------------------------------------------

/// What was learned from a correction.
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedCorrection {
    /// The correct spelling (the user's selection).
    pub word: String,
    /// The misheard text found in the last dictation, if any.
    pub misheard: Option<String>,
}

fn clean_token(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
}

/// Normalize a user selection into a correction candidate, or `None` when the
/// selection is empty or too long to be a word/name correction.
pub fn normalize_selection(selection: &str) -> Option<String> {
    let words: Vec<&str> = selection
        .split_whitespace()
        .map(clean_token)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() || words.len() > MAX_CORRECTION_WORDS {
        return None;
    }
    let word = words.join(" ");
    if word.chars().count() > MAX_CORRECTION_CHARS {
        return None;
    }
    Some(word)
}

/// Compare a corrected word with the last dictated text and find the n-gram
/// that was most likely misheard.
pub fn learn_correction(previous: &str, selection: &str) -> Option<LearnedCorrection> {
    let word = normalize_selection(selection)?;
    let target = word.to_lowercase();
    let target_words = word.split_whitespace().count();

    let tokens: Vec<&str> = previous
        .split_whitespace()
        .map(clean_token)
        .filter(|w| !w.is_empty())
        .collect();

    // The misheard version can have a different word count ("chat GPT" for
    // "ChatGPT"), so try n-grams a bit shorter and longer than the selection.
    let min_n = target_words.saturating_sub(1).max(1);
    let max_n = (target_words + 2).min(MAX_CORRECTION_WORDS + 1);

    let mut best: Option<(f64, String)> = None;
    for n in min_n..=max_n {
        if n > tokens.len() {
            break;
        }
        for window in tokens.windows(n) {
            let candidate = window.join(" ");
            let lowered = candidate.to_lowercase();
            if lowered == target {
                // Already dictated correctly (maybe with different casing).
                let misheard = (candidate != word).then_some(candidate);
                return Some(LearnedCorrection { word, misheard });
            }
            // Compare without spaces so "chat GPT" matches "ChatGPT" well.
            let score =
                strsim::normalized_levenshtein(&lowered.replace(' ', ""), &target.replace(' ', ""));
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, candidate));
            }
        }
    }

    let misheard = best
        .filter(|(score, _)| *score >= MIN_MISHEARD_SIMILARITY)
        .map(|(_, candidate)| candidate);
    Some(LearnedCorrection { word, misheard })
}

/// Fold a learned correction into the user's vocabulary. Returns `true` when
/// anything changed.
pub fn apply_learned_correction(
    learned: &LearnedCorrection,
    custom_words: &mut Vec<String>,
    replacements: &mut Vec<TextReplacement>,
) -> bool {
    let mut changed = false;
    if !custom_words
        .iter()
        .any(|w| w.eq_ignore_ascii_case(&learned.word))
    {
        custom_words.push(learned.word.clone());
        changed = true;
    }

    if let Some(misheard) = &learned.misheard {
        if let Some(existing) = replacements
            .iter_mut()
            .find(|r| r.from.eq_ignore_ascii_case(misheard))
        {
            if existing.to != learned.word {
                existing.to = learned.word.clone();
                existing.learned = true;
                changed = true;
            }
        } else {
            replacements.push(TextReplacement {
                from: misheard.clone(),
                to: learned.word.clone(),
                learned: true,
            });
            changed = true;
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// Text replacements
// ---------------------------------------------------------------------------

fn replacement_regex(from: &str) -> Option<Regex> {
    let from = from.trim();
    if from.is_empty() {
        return None;
    }
    // Word boundaries only where the phrase starts/ends with a word character,
    // so replacements like "->" still work.
    let starts_word = from.chars().next().is_some_and(|c| c.is_alphanumeric());
    let ends_word = from.chars().last().is_some_and(|c| c.is_alphanumeric());
    let escaped = regex::escape(from).replace(' ', r"\s+");
    let pattern = format!(
        "{}{}{}",
        if starts_word { r"\b" } else { "" },
        escaped,
        if ends_word { r"\b" } else { "" }
    );
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .unicode(true)
        .build()
        .ok()
}

/// Apply `from → to` replacements, longest `from` first so that a phrase wins
/// over a single word it contains. A literal `\n` in `to` becomes a newline.
pub fn apply_text_replacements(text: &str, replacements: &[TextReplacement]) -> String {
    if replacements.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let mut ordered: Vec<&TextReplacement> = replacements.iter().collect();
    ordered.sort_by_key(|r| std::cmp::Reverse(r.from.trim().chars().count()));

    let mut result = text.to_string();
    for replacement in ordered {
        if let Some(re) = replacement_regex(&replacement.from) {
            let to = replacement.to.replace("\\n", "\n");
            result = re.replace_all(&result, regex::NoExpand(&to)).into_owned();
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Per-app prompts
// ---------------------------------------------------------------------------

/// Pick the prompt for the focused app: the first rule whose `app_match`
/// (case-insensitive, `|`-separated alternatives) appears in the app name or
/// window title.
pub fn prompt_id_for_app<'a>(rules: &'a [AppPromptRule], app: &ActiveApp) -> Option<&'a str> {
    let name = app.name.to_lowercase();
    let title = app.title.to_lowercase();
    rules
        .iter()
        .find(|rule| {
            rule.app_match
                .split('|')
                .map(|alt| alt.trim().to_lowercase())
                .filter(|alt| !alt.is_empty())
                .any(|alt| name.contains(&alt) || title.contains(&alt))
        })
        .map(|rule| rule.prompt_id.as_str())
}

/// Add the user's context to a post-processing prompt: replaces `${app}` and
/// appends the personal vocabulary so the LLM keeps those spellings.
pub fn personalize_prompt(
    prompt: &str,
    app: Option<&ActiveApp>,
    custom_words: &[String],
) -> String {
    let app_label = app.map(ActiveApp::label).unwrap_or_default();
    let mut prompt = prompt.replace("${app}", &app_label);
    if !custom_words.is_empty() {
        prompt.push_str(
            "\n\nThe user's personal vocabulary (always spell these exactly as written): ",
        );
        prompt.push_str(&custom_words.join(", "));
    }
    prompt
}

// ---------------------------------------------------------------------------
// Active window detection
// ---------------------------------------------------------------------------

/// The currently focused application, or `None` when it can't be determined
/// (e.g. on unsupported Wayland compositors).
pub fn active_app() -> Option<ActiveApp> {
    let app = platform::active_app()?;
    (!app.name.is_empty() || !app.title.is_empty()).then_some(app)
}

#[cfg(target_os = "windows")]
mod platform {
    use super::ActiveApp;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    pub fn active_app() -> Option<ActiveApp> {
        // SAFETY: plain Win32 calls; every buffer passed is a live local array
        // and the process handle is closed before returning.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }

            let mut title_buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut title_buf);
            let title = String::from_utf16_lossy(&title_buf[..len.max(0) as usize]);

            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut name = String::new();
            if pid != 0 {
                if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut path_buf = [0u16; 1024];
                    let mut size = path_buf.len() as u32;
                    if QueryFullProcessImageNameW(
                        process,
                        PROCESS_NAME_WIN32,
                        PWSTR(path_buf.as_mut_ptr()),
                        &mut size,
                    )
                    .is_ok()
                    {
                        let path = String::from_utf16_lossy(&path_buf[..size as usize]);
                        name = std::path::Path::new(&path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                    let _ = CloseHandle(process);
                }
            }
            Some(ActiveApp { name, title })
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::ActiveApp;
    use objc2_app_kit::NSWorkspace;

    pub fn active_app() -> Option<ActiveApp> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let name = app
            .localizedName()
            .map(|n| n.to_string())
            .unwrap_or_default();
        // The window title needs the Screen Recording permission; the app
        // name is enough to pick a prompt.
        Some(ActiveApp {
            name,
            title: String::new(),
        })
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::ActiveApp;
    use std::process::Command;

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(cmd).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    /// Hyprland exposes the focused window over its CLI.
    fn hyprland() -> Option<ActiveApp> {
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
        let json = run("hyprctl", &["activewindow", "-j"])?;
        let value: serde_json::Value = serde_json::from_str(&json).ok()?;
        Some(ActiveApp {
            name: value["class"].as_str().unwrap_or_default().to_string(),
            title: value["title"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// X11 (and XWayland apps) through `xprop`.
    fn x11() -> Option<ActiveApp> {
        std::env::var_os("DISPLAY")?;
        let root = run("xprop", &["-root", "_NET_ACTIVE_WINDOW"])?;
        let id = root.split_whitespace().last()?.trim_end_matches(',');
        if id == "0x0" {
            return None;
        }
        let props = run("xprop", &["-id", id, "WM_CLASS", "_NET_WM_NAME"])?;
        let mut app = ActiveApp::default();
        for line in props.lines() {
            let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
            if line.starts_with("WM_CLASS") {
                // WM_CLASS = "instance", "Class" — the class reads better.
                app.name = quoted.last().copied().unwrap_or_default().to_string();
            } else if line.starts_with("_NET_WM_NAME") {
                app.title = quoted.first().copied().unwrap_or_default().to_string();
            }
        }
        Some(app)
    }

    pub fn active_app() -> Option<ActiveApp> {
        hyprland().or_else(x11)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn active_app() -> Option<super::ActiveApp> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(app_match: &str, prompt_id: &str) -> AppPromptRule {
        AppPromptRule {
            app_match: app_match.to_string(),
            prompt_id: prompt_id.to_string(),
        }
    }

    fn replacement(from: &str, to: &str) -> TextReplacement {
        TextReplacement {
            from: from.to_string(),
            to: to.to_string(),
            learned: false,
        }
    }

    #[test]
    fn learns_misheard_single_word() {
        let learned = learn_correction("J'ai parlé avec Tomas hier", "Thomas").unwrap();
        assert_eq!(learned.word, "Thomas");
        assert_eq!(learned.misheard.as_deref(), Some("Tomas"));
    }

    #[test]
    fn learns_split_word() {
        let learned = learn_correction("ask chat GPT about it", "ChatGPT").unwrap();
        assert_eq!(learned.misheard.as_deref(), Some("chat GPT"));
    }

    #[test]
    fn learns_casing_fix() {
        let learned = learn_correction("deploy on github today", "GitHub").unwrap();
        assert_eq!(learned.misheard.as_deref(), Some("github"));
    }

    #[test]
    fn exact_word_has_nothing_misheard() {
        let learned = learn_correction("open Wisprfree now", "Wisprfree").unwrap();
        assert_eq!(learned.misheard, None);
    }

    #[test]
    fn unrelated_selection_only_adds_word() {
        let learned = learn_correction("bonjour à tous", "Kubernetes").unwrap();
        assert_eq!(learned.word, "Kubernetes");
        assert_eq!(learned.misheard, None);
    }

    #[test]
    fn selection_is_trimmed_and_bounded() {
        assert_eq!(
            normalize_selection("  «Thomas», ").as_deref(),
            Some("Thomas")
        );
        assert_eq!(normalize_selection("   "), None);
        assert_eq!(normalize_selection("one two three four five"), None);
    }

    #[test]
    fn applying_correction_updates_vocabulary_once() {
        let learned = LearnedCorrection {
            word: "Thomas".to_string(),
            misheard: Some("Tomas".to_string()),
        };
        let mut words = vec![];
        let mut replacements = vec![];
        assert!(apply_learned_correction(
            &learned,
            &mut words,
            &mut replacements
        ));
        assert!(!apply_learned_correction(
            &learned,
            &mut words,
            &mut replacements
        ));
        assert_eq!(words, vec!["Thomas".to_string()]);
        assert_eq!(replacements.len(), 1);
        assert!(replacements[0].learned);
    }

    #[test]
    fn replacements_respect_word_boundaries_and_case() {
        let out = apply_text_replacements(
            "Tomas et Tomasz parlent à tomas",
            &[replacement("Tomas", "Thomas")],
        );
        assert_eq!(out, "Thomas et Tomasz parlent à Thomas");
    }

    #[test]
    fn longer_replacements_win_and_newlines_expand() {
        let out = apply_text_replacements(
            "voici mon adresse complète merci",
            &[
                replacement("adresse", "ADR"),
                replacement("mon adresse complète", "12 rue X\\n75000 Paris"),
            ],
        );
        assert_eq!(out, "voici 12 rue X\n75000 Paris merci");
    }

    #[test]
    fn replacement_text_is_literal() {
        let out = apply_text_replacements("price", &[replacement("price", "$1 $2")]);
        assert_eq!(out, "$1 $2");
    }

    #[test]
    fn picks_prompt_from_app_name_or_title() {
        let rules = vec![
            rule("slack|discord", "chat"),
            rule("gmail|outlook", "email"),
        ];
        let slack = ActiveApp {
            name: "Slack".into(),
            title: String::new(),
        };
        let gmail = ActiveApp {
            name: "chrome".into(),
            title: "Inbox - Gmail".into(),
        };
        let code = ActiveApp {
            name: "Code".into(),
            title: String::new(),
        };
        assert_eq!(prompt_id_for_app(&rules, &slack), Some("chat"));
        assert_eq!(prompt_id_for_app(&rules, &gmail), Some("email"));
        assert_eq!(prompt_id_for_app(&rules, &code), None);
    }

    #[test]
    fn prompt_is_personalized() {
        let app = ActiveApp {
            name: "Slack".into(),
            title: String::new(),
        };
        let prompt = personalize_prompt(
            "Write for ${app}.",
            Some(&app),
            &["Thomas".to_string(), "Wisprfree".to_string()],
        );
        assert!(prompt.starts_with("Write for Slack."));
        assert!(prompt.ends_with("Thomas, Wisprfree"));
    }
}
