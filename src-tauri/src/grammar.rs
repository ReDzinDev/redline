use std::sync::{Arc, Mutex};
use std::time::Instant;

use harper_core::language_detection::is_doc_likely_english;
use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::spell::{Dictionary, FstDictionary};
use harper_core::{Dialect, Document, LintContext, Span, TokenKind, remove_overlaps};

use crate::ai::prompts::normalize_line_endings;
use crate::dto::{LintDto, LintResponseDto, SuggestionDto};

struct LinterState {
    dialect: Dialect,
    dict: Arc<FstDictionary>,
    group: LintGroup,
}

/// Upstream `harper-core 3.2.0` rules that execute unguarded `eprintln!`/`println!` printing
/// verbatim user text clauses to stdout/stderr. Disabled to preserve Redline's zero-user-text
/// diagnostic guarantee.
const NOISY_STDIO_HARPER_RULES: &[&str] = &[
    "SeemsLikeAdjective",
    "ExpandIme",
    "ChickenAndEgg",
    "LookingForwardTo",
];

fn build_curated_lint_group(dict: Arc<FstDictionary>, dialect: Dialect) -> LintGroup {
    let mut group = LintGroup::new_curated(dict, dialect);
    for &rule in NOISY_STDIO_HARPER_RULES {
        group.config.set_rule_enabled(rule, false);
    }
    group
}

/// Manages a lazily built Harper `LintGroup` behind a `Mutex`.
/// Changing dialect replaces the `LintGroup` while reusing the curated `FstDictionary`.
pub struct GrammarEngine {
    inner: Mutex<Option<LinterState>>,
    desired_dialect: Mutex<Dialect>,
}

impl GrammarEngine {
    pub fn new(initial_dialect: &str) -> Self {
        Self {
            inner: Mutex::new(None),
            desired_dialect: Mutex::new(parse_dialect(initial_dialect)),
        }
    }

    pub fn set_dialect(&self, dialect_name: &str) {
        let next = parse_dialect(dialect_name);
        *self
            .desired_dialect
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;

        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = guard.as_mut()
            && state.dialect != next
        {
            state.group = build_curated_lint_group(Arc::clone(&state.dict), next);
            state.dialect = next;
            log::info!("rebuilt Harper linter for dialect={dialect_name}");
        }
    }

    pub fn lint(&self, raw_text: &str, request_id: u64) -> LintResponseDto {
        let started = Instant::now();
        let (normalized, _) = normalize_line_endings(raw_text);
        if normalized.trim().is_empty() {
            return LintResponseDto {
                request_id,
                english: true,
                lints: Vec::new(),
            };
        }

        let target_dialect = *self
            .desired_dialect
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let mut guard = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let state = guard.get_or_insert_with(|| {
            let init_start = Instant::now();
            let dict = FstDictionary::curated();
            let group = build_curated_lint_group(Arc::clone(&dict), target_dialect);
            log::info!(
                "initialized Harper linter in {} ms",
                init_start.elapsed().as_millis()
            );
            LinterState {
                dialect: target_dialect,
                dict,
                group,
            }
        });

        if state.dialect != target_dialect {
            state.group = build_curated_lint_group(Arc::clone(&state.dict), target_dialect);
            state.dialect = target_dialect;
        }

        let doc = Document::new_plain_english(&normalized, state.dict.as_ref());
        if !is_text_likely_english(&doc, state.dict.as_ref()) {
            log::debug!(
                "Harper skipped non-English text: chars={} elapsed_ms={}",
                normalized.chars().count(),
                started.elapsed().as_millis()
            );
            return LintResponseDto {
                request_id,
                english: false,
                lints: Vec::new(),
            };
        }

        let mut raw_lints = state.group.lint(&doc);
        remove_overlaps(&mut raw_lints);

        // Sort deterministically by start ascending, then longest span first, and strip any
        // remaining overlaps defensively so Fix all in reverse order never drifts.
        raw_lints.sort_by(|a, b| {
            a.span
                .start
                .cmp(&b.span.start)
                .then_with(|| b.span.len().cmp(&a.span.len()))
        });

        let source = doc.get_source();
        let char_to_utf16 = build_utf16_offset_table(source);

        let mut lints = Vec::with_capacity(raw_lints.len());
        let mut last_end = 0usize;

        for lint in raw_lints {
            if lint.span.start < last_end || lint.span.end > source.len() {
                continue;
            }
            last_end = lint.span.end;

            let flagged = lint.get_str(source);
            let span_chars = &source[lint.span.start..lint.span.end];
            let suggestions = lint
                .suggestions
                .iter()
                .map(|sug| suggestion_to_dto(sug, span_chars))
                .collect();

            let id = LintContext::from_lint(&lint, &doc)
                .default_hash()
                .to_string();

            lints.push(LintDto {
                id,
                start: char_to_utf16[lint.span.start],
                end: char_to_utf16[lint.span.end],
                flagged,
                category: classify_lint_kind(lint.lint_kind).to_string(),
                kind: format!("{:?}", lint.lint_kind),
                message: lint.message,
                suggestions,
            });
        }

        log::debug!(
            "Harper lint complete: chars={} lints={} elapsed_ms={}",
            normalized.chars().count(),
            lints.len(),
            started.elapsed().as_millis()
        );

        LintResponseDto {
            request_id,
            english: true,
            lints,
        }
    }
}

pub fn parse_dialect(name: &str) -> Dialect {
    match name.trim().to_ascii_lowercase().as_str() {
        "british" | "gb" | "uk" => Dialect::British,
        "canadian" | "ca" => Dialect::Canadian,
        "australian" | "au" => Dialect::Australian,
        "indian" | "in" => Dialect::Indian,
        _ => Dialect::American,
    }
}

/// Language rule from §4:
/// - 0 words -> treat as English (nothing to flag).
/// - 8+ words -> Harper's `is_doc_likely_english`.
/// - 1..=7 words -> English if at least half of the words are in the dictionary ("Teh cat sat" passes).
pub fn is_text_likely_english(doc: &Document, dict: &impl Dictionary) -> bool {
    let source = doc.get_source();
    let mut total_words = 0usize;
    let mut valid_words = 0usize;

    for token in doc.get_tokens() {
        if matches!(token.kind, TokenKind::Word(_)) {
            total_words += 1;
            if dict.contains_word(token.get_ch(source)) {
                valid_words += 1;
            }
        }
    }

    if total_words == 0 {
        return true;
    }

    if total_words >= 8 {
        return is_doc_likely_english(doc, dict);
    }

    valid_words * 2 >= total_words
}

fn build_utf16_offset_table(source: &[char]) -> Vec<usize> {
    let mut table = Vec::with_capacity(source.len() + 1);
    let mut utf16 = 0usize;
    table.push(0);
    for &ch in source {
        utf16 += ch.len_utf16();
        table.push(utf16);
    }
    table
}

fn suggestion_to_dto(suggestion: &Suggestion, span_chars: &[char]) -> SuggestionDto {
    let mut buf = span_chars.to_vec();
    suggestion.apply(Span::new(0, buf.len()), &mut buf);
    let replacement: String = buf.into_iter().collect();

    let label = match suggestion {
        Suggestion::Remove => "Remove".to_string(),
        Suggestion::ReplaceWith(chars) => chars.iter().collect(),
        Suggestion::InsertAfter(_) => replacement.clone(),
    };

    SuggestionDto { label, replacement }
}

fn classify_lint_kind(kind: LintKind) -> &'static str {
    match kind {
        LintKind::Spelling | LintKind::Typo => "spelling",
        LintKind::Grammar
        | LintKind::Agreement
        | LintKind::BoundaryError
        | LintKind::Capitalization
        | LintKind::Punctuation
        | LintKind::Eggcorn
        | LintKind::Malapropism
        | LintKind::WordOrder
        | LintKind::Repetition => "grammar",
        _ => "style",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_handle_emoji_and_accents_accurately() {
        let engine = GrammarEngine::new("american");
        let text = "🚀 Café teh quick brown fox.";
        let res = engine.lint(text, 1);
        assert!(res.english);
        let teh_lint = res
            .lints
            .iter()
            .find(|l| l.flagged.eq_ignore_ascii_case("teh"))
            .expect("expected lint for 'teh'");

        let utf16: Vec<u16> = text.encode_utf16().collect();
        let sliced = String::from_utf16(&utf16[teh_lint.start..teh_lint.end]).expect("valid utf16");
        assert_eq!(sliced, teh_lint.flagged);
        // 🚀 (2) + ' ' (1) + "Café" (4) + ' ' (1) = 8
        assert_eq!(teh_lint.start, 8);
        assert_eq!(teh_lint.end, 11);
    }

    #[test]
    fn suggestion_apply_converts_all_variants_to_span_replacements() {
        let span: Vec<char> = "teh".chars().collect();
        let replace = suggestion_to_dto(&Suggestion::ReplaceWith("the".chars().collect()), &span);
        assert_eq!(replace.replacement, "the");
        assert_eq!(replace.label, "the");

        let insert = suggestion_to_dto(&Suggestion::InsertAfter("!".chars().collect()), &span);
        assert_eq!(insert.replacement, "teh!");

        let remove = suggestion_to_dto(&Suggestion::Remove, &span);
        assert_eq!(remove.replacement, "");
        assert_eq!(remove.label, "Remove");
    }

    #[test]
    fn language_rule_accepts_short_english_with_typo_and_skips_non_english() {
        let engine = GrammarEngine::new("american");

        let short_en = engine.lint("Teh cat sat", 1);
        assert!(short_en.english);
        assert!(
            short_en
                .lints
                .iter()
                .any(|l| l.flagged.eq_ignore_ascii_case("teh")),
            "expected typo lint for 'Teh cat sat'"
        );

        let short_pt = engine.lint("Olá meu amigo", 2);
        assert!(!short_pt.english);
        assert!(short_pt.lints.is_empty());

        let long_es = engine.lint(
            "Esto es un texto en español para verificar que no se marque como inglés.",
            3,
        );
        assert!(!long_es.english);
        assert!(long_es.lints.is_empty());
    }

    #[test]
    fn lint_id_survives_relint_after_earlier_edit() {
        let engine = GrammarEngine::new("american");
        let first = engine.lint("Hello world. Teh cat sat on the mat.", 1);
        let second = engine.lint("Hi there, world. Teh cat sat on the mat.", 2);

        let id1 = first
            .lints
            .iter()
            .find(|l| l.flagged == "Teh")
            .expect("first Teh lint")
            .id
            .clone();
        let id2 = second
            .lints
            .iter()
            .find(|l| l.flagged == "Teh")
            .expect("second Teh lint")
            .id
            .clone();
        assert_eq!(id1, id2);
    }

    #[test]
    fn dialect_change_rebuilds_linter_cleanly() {
        let engine = GrammarEngine::new("american");
        let us = engine.lint("The colour of the sky is blue.", 1);
        assert!(us.english);
        assert!(us.lints.iter().any(|l| l.flagged == "colour"));

        engine.set_dialect("british");
        let uk = engine.lint("The colour of the sky is blue.", 2);
        assert!(uk.english);
        assert!(!uk.lints.iter().any(|l| l.flagged == "colour"));
    }

    #[test]
    fn lint_12k_char_document_produces_valid_non_overlapping_utf16_spans() {
        let engine = GrammarEngine::new("american");
        let paragraph = "The quick brown fox jumps over teh lazy dog near the quiet river. ";
        let mut text = String::with_capacity(12_100);
        while text.len() < 12_000 {
            text.push_str(paragraph);
        }
        text.truncate(12_000);

        let res = engine.lint(&text, 42);
        assert!(res.english);
        assert!(!res.lints.is_empty());

        let utf16: Vec<u16> = text.encode_utf16().collect();
        let mut prev_end = 0usize;
        for lint in &res.lints {
            assert!(lint.start >= prev_end, "lint spans must not overlap");
            assert!(lint.end <= utf16.len(), "lint end out of UTF-16 bounds");
            let sliced = String::from_utf16(&utf16[lint.start..lint.end]).expect("valid utf16");
            assert_eq!(sliced, lint.flagged);
            prev_end = lint.end;
        }
    }

    #[test]
    fn disables_noisy_stdio_harper_rules() {
        let dict = FstDictionary::curated();
        let group = build_curated_lint_group(dict, Dialect::American);
        for &rule in NOISY_STDIO_HARPER_RULES {
            assert!(
                !group.config.is_rule_enabled(rule),
                "expected noisy Harper rule {rule} to be disabled"
            );
        }
    }
}
