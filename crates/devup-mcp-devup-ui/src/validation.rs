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
    if parsed.errors.is_empty() {
        return Ok(TsxValidation {
            byte_len: source.len(),
            statement_count: parsed.program.body.len(),
        });
    }
    let errors = parsed
        .errors
        .iter()
        .map(|error| {
            let (start, end) = error
                .labels
                .as_ref()
                .and_then(|labels| labels.first())
                .map(|label| {
                    let start = label.offset().min(source.len());
                    let end = start.saturating_add(label.len()).min(source.len());
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
        "생성된 DevupUI TSX가 TypeScript JSX 문법 검증을 통과하지 못했습니다.",
        false,
        json!({
            "errorCount": errors.len(),
            "panicked": parsed.panicked,
            "errors": errors
        }),
    ))
}
