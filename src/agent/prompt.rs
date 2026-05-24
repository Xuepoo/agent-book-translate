//! Sliding window prompt & POV metadata generator.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct GlossaryPayload {
    pub new_entities: Vec<NewEntity>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct NewEntity {
    pub original_name: String,
    pub translated_name: String,
    pub category: String,
    pub profile: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CritiqueReport {
    pub has_mismatches: bool,
    pub incorrect_terms: Vec<TermMismatch>,
    pub refined_translation: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct TermMismatch {
    pub term: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub book_summary: String,
    pub pov_speaker: String,
    pub glossary: Vec<NewEntity>,
    pub previous_context: String,
    pub target: String,
    pub next_context: String,
}

pub fn build_translation_prompt(ctx: &PromptContext) -> String {
    let glossary = if ctx.glossary.is_empty() {
        String::new()
    } else {
        ctx.glossary
            .iter()
            .map(|item| {
                format!(
                    "- {} => {} ({}) {}",
                    item.original_name, item.translated_name, item.category, item.profile
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "You are a master translator specializing in literary works.\n\n[Global Background Context]\n<book_summary>\n{}\n</book_summary>\n\n[Current POV & Tone Constraint]\n<pov_speaker>\n{}\n</pov_speaker>\n\n[Local Character & Term Glossary References]\n<glossary>\n{}\n</glossary>\n\n[Semantic Context Alignment]\n<previous_context>\n{}\n</previous_context>\n\n<target>\n{}\n</target>\n\n<next_context>\n{}\n</next_context>\n",
        ctx.book_summary,
        ctx.pov_speaker,
        glossary,
        ctx.previous_context,
        ctx.target,
        ctx.next_context
    )
}
