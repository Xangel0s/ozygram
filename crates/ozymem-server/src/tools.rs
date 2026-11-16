use serde_json::Value;

pub(crate) fn mark_legacy_tools_deprecated(value: &mut Value) {
    let Some(tools) = value.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("");
        if name.starts_with("ozy_") {
            continue;
        }
        if let Some(desc) = tool.get("description").and_then(Value::as_str) {
            let new_desc = format!("[DEPRECATED: use one of the 5 unified ozy_* meta-tools] {desc}");
            tool["description"] = Value::String(new_desc);
        }
        tool["deprecated"] = Value::Bool(true);
    }
}

pub(crate) fn handle_lookup_engram(
    backend: &ozymem_core::graph_backend::GraphBackend,
    tool_call: &ozymem_core::mcp_common::ToolCallParams,
) -> anyhow::Result<ozymem_core::mcp_common::ToolCallResult> {
    use ozymem_core::mcp_common::{ContentBlock, ToolCallResult};
    use crate::formatters::format_engram_contract;

    let symbol_path = tool_call
        .arguments
        .get("symbol_path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing 'symbol_path'"))?;

    // 1. Zero-Copy direct mmap lookup (< 25 ns)
    if let Some(text) = backend.engram_store.with_archived(symbol_path, |archived| {
        let mut out = "Found 1 engram contract(s) in O(1) time (Zero-Copy mmap):\n\n".to_string();
        out.push_str(&crate::formatters::format_archived_engram_contract(archived));
        out
    }) {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text,
            }],
            is_error: None,
        });
    }

    let mut contracts = Vec::new();
    if let Some(c) = backend.engram_store.lookup(symbol_path) {
        contracts.push(c);
    } else {
        contracts.extend(backend.engram_store.lookup_by_name(symbol_path));
    }

    let body = if contracts.is_empty() {
        format!("No deterministic engram contract found for '{symbol_path}'.")
    } else {
        let mut text = format!("Found {} engram contract(s) in O(1) time:\n\n", contracts.len());
        for c in &contracts {
            text.push_str(&format_engram_contract(c));
            text.push('\n');
        }
        text
    };

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: body,
        }],
        is_error: None,
    })
}

pub(crate) fn handle_read_blob(
    backend: &ozymem_core::graph_backend::GraphBackend,
    tool_call: &ozymem_core::mcp_common::ToolCallParams,
) -> anyhow::Result<ozymem_core::mcp_common::ToolCallResult> {
    use ozymem_core::mcp_common::{ContentBlock, ToolCallResult};
    use std::path::Path;

    let blob_id = tool_call
        .arguments
        .get("blob_id")
        .or_else(|| tool_call.arguments.get("uri"))
        .or_else(|| tool_call.arguments.get("path"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing required parameter 'blob_id'"))?;

    let offset = tool_call
        .arguments
        .get("offset")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok())))
        .unwrap_or(0) as usize;

    let limit = tool_call
        .arguments
        .get("limit")
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok())))
        .unwrap_or(200) as usize;

    let hash = if let Some(stripped) = blob_id.strip_prefix("blob://") {
        stripped.trim_start_matches(".ozymem/blobs/").trim_start_matches(".ozymem\\blobs\\").trim_end_matches(".txt")
    } else if let Some(stripped) = blob_id.strip_prefix("ozymem://blob/") {
        stripped.trim_end_matches(".txt")
    } else {
        blob_id.trim_start_matches(".ozymem/blobs/").trim_start_matches(".ozymem\\blobs\\").trim_end_matches(".txt")
    };

    let blob_dir = match backend.project_path() {
        Some(root) => Path::new(&root).join(".ozymem").join("blobs"),
        None => Path::new(".ozymem").join("blobs"),
    };

    let target_file = blob_dir.join(format!("{hash}.txt"));
    let target_file = if target_file.exists() {
        target_file
    } else {
        blob_dir.join(blob_id)
    };

    if !target_file.exists() {
        return Ok(ToolCallResult {
            content: vec![ContentBlock {
                kind: "text",
                text: format!("[ALERT: BLOB_NOT_FOUND] Blob '{blob_id}' not found in {}", blob_dir.display()),
            }],
            is_error: Some(true),
        });
    }

    let full_text = std::fs::read_to_string(&target_file)?;
    let lines: Vec<&str> = full_text.lines().collect();
    let total_lines = lines.len();

    let start = offset.min(total_lines);
    let end = (offset + limit).min(total_lines);
    let slice = &lines[start..end];
    let has_more = end < total_lines;

    let mut body = format!(
        "[OZY_BLOB: {}] (Lines {}-{} of {}, has_more: {})\n\n",
        hash,
        if total_lines == 0 { 0 } else { start + 1 },
        end,
        total_lines,
        has_more
    );
    body.push_str(&slice.join("\n"));

    if has_more {
        body.push_str(&format!(
            "\n\n[NOTE: Use ozy_read_blob(blob_id=\"{}\", offset={}, limit={}) to continue reading.]",
            hash,
            end,
            limit
        ));
    }

    Ok(ToolCallResult {
        content: vec![ContentBlock {
            kind: "text",
            text: body,
        }],
        is_error: None,
    })
}


