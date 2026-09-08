use devup_mcp_figma::{DevupError, ErrorCode};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TsxValidation {
    pub byte_len: usize,
    pub statement_count: usize,
}

pub fn validate_tsx(source: &str) -> Result<TsxValidation, DevupError> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::tsx()).parse();
    // `ParserReturn::diagnostics` (oxc_parser 0.148, was `errors` on 0.96) derefs to
    // `Vec<OxcDiagnostic>`, so this keeps the original "any diagnostic fails validation"
    // behavior regardless of severity.
    if parsed.diagnostics.is_empty() {
        return Ok(TsxValidation {
            byte_len: source.len(),
            statement_count: parsed.program.body.len(),
        });
    }
    let errors = parsed
        .diagnostics
        .iter()
        .map(|error| {
            // `OxcDiagnosticInner::labels` (oxc_diagnostics 0.148) is a plain `Labels`
            // collection (was `Option<Vec<LabeledSpan>>` on 0.96), and
            // `LabeledSpan::offset`/`len` now return `u32` (was `usize`).
            let (start, end) = error
                .labels
                .first()
                .map(|label| {
                    let start = (label.offset() as usize).min(source.len());
                    let end = start.saturating_add(label.len() as usize).min(source.len());
                    (start, end)
                })
                .unwrap_or((0, 0));
            json!({
                "category": "syntax",
                "start": start,
                "end": end
            })
        })
        .collect::<Vec<_>>();
    Err(DevupError::with_details(
        ErrorCode::DevupCodegenFailed,
        "Generated DevupUI TSX failed TypeScript JSX syntax validation.",
        false,
        json!({
            "errorCount": errors.len(),
            "panicked": parsed.panicked,
            "errors": errors
        }),
    ))
}
