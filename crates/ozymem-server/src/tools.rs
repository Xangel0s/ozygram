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
            let new_desc = format!("[DEPRECATED: use one of the 7 unified ozy_* tools] {desc}");
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

