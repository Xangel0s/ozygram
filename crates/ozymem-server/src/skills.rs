use serde_json::Value;
use ozymem_core::mcp_common::{self, ContentBlock, ToolCallResult};

pub(crate) async fn handle_ozy_skills(
    tool_call: &mcp_common::ToolCallParams,
) -> anyhow::Result<ToolCallResult> {
    let action = tool_call
        .arguments
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    let query = tool_call
        .arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let limit = tool_call
        .arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20) as usize;
    let mut skills = official_skill_seed();
    if !query.is_empty() {
        skills.retain(|s| {
            s.get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase()
                .contains(&query)
                || s.get("repo")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
                || s.get("category")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
        });
    }
    skills.truncate(limit);
    let payload = match action {
        "sync" => {
            serde_json::json!({"source":"https://www.skills.sh/official","mode":"metadata_seed","executed_external_content":false,"imported":skills})
        }
        "apply" => {
            serde_json::json!({"source":"skills.sh official metadata","mode":"checklist_only","query":query,"checklist":["Identify relevant official skill category before coding.","Prefer framework/vendor guidance over generic rules.","Use Ozy Doctor Code findings as evidence, not automatic edits.","Keep all external skill content review-only unless explicitly approved."],"matches":skills})
        }
        _ => {
            serde_json::json!({"source":"https://www.skills.sh/official","official_only":true,"skills":skills})
        }
    };
    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: serde_json::to_string_pretty(&payload)?,
        }],
        is_error: None,
    })
}

pub(crate) fn official_skill_seed() -> Vec<Value> {
    vec![
        serde_json::json!({"name":"react","repo":"facebook/react","category":"frontend","origin":"skills.sh/official","audit_status":"external_review_required"}),
        serde_json::json!({"name":"vercel-ai","repo":"vercel/ai","category":"ai-sdk","origin":"skills.sh/official","audit_status":"external_review_required"}),
        serde_json::json!({"name":"openai-skills","repo":"openai/skills","category":"openai","origin":"skills.sh/official","audit_status":"external_review_required"}),
        serde_json::json!({"name":"supabase-agent-skills","repo":"supabase/agent-skills","category":"database","origin":"skills.sh/official","audit_status":"external_review_required"}),
        serde_json::json!({"name":"prisma-skills","repo":"prisma/skills","category":"database","origin":"skills.sh/official","audit_status":"external_review_required"}),
        serde_json::json!({"name":"semgrep-skills","repo":"semgrep/skills","category":"security","origin":"skills.sh/official","audit_status":"external_review_required"}),
        serde_json::json!({"name":"getsentry-skills","repo":"getsentry/skills","category":"observability","origin":"skills.sh/official","audit_status":"external_review_required"}),
    ]
}

