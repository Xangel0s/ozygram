import os
import tempfile
import pytest
from ozy_brain.ast_preflight import extract_file_contract_skeleton, _CONTRACT_SKELETON_CACHE
from ozy_brain.planner import plan


def test_extract_python_contracts():
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False, encoding="utf-8") as f:
        f.write("""
class UserModel:
    pass

def get_user(id: int):
    pass

async def update_user(id: int):
    pass

def _private_helper():
    pass
""")
        temp_path = f.name

    try:
        symbols = extract_file_contract_skeleton(temp_path, max_symbols=4)
        assert "UserModel" in symbols
        assert "get_user" in symbols
        assert "update_user" in symbols
        assert "_private_helper" not in symbols
    finally:
        if os.path.exists(temp_path):
            os.remove(temp_path)


def test_extract_typescript_tsx_contracts():
    with tempfile.NamedTemporaryFile("w", suffix=".tsx", delete=False, encoding="utf-8") as f:
        f.write("""
export interface TableEditorProps {
    tableId: string;
}

export function TableEditor({ tableId }: TableEditorProps) {
    return null;
}

export const useTableCache = () => {};

export type TableRowState = "clean" | "dirty";
""")
        temp_path = f.name

    try:
        symbols = extract_file_contract_skeleton(temp_path, max_symbols=4)
        assert "TableEditorProps" in symbols
        assert "TableEditor" in symbols
        assert "useTableCache" in symbols
        assert "TableRowState" in symbols
    finally:
        if os.path.exists(temp_path):
            os.remove(temp_path)


def test_extract_rust_contracts():
    with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False, encoding="utf-8") as f:
        f.write("""
pub struct AmygdalaReflexEntry {
    pub id: String,
}

pub async fn trigger_reflex() {}

pub enum ThreatLevel { Low, High }

pub trait MemoryEngine {}
""")
        temp_path = f.name

    try:
        symbols = extract_file_contract_skeleton(temp_path, max_symbols=4)
        assert "AmygdalaReflexEntry" in symbols
        assert "trigger_reflex" in symbols
        assert "ThreatLevel" in symbols
        assert "MemoryEngine" in symbols
    finally:
        if os.path.exists(temp_path):
            os.remove(temp_path)


def test_contract_preview_injected_in_planner_steps():
    # Use existing python file from ozy_brain as candidate
    target_py = "python/ozy-brain/ozy_brain/ast_preflight.py"
    payload = {
        "goal": "Refactor AST preflight validators",
        "candidate_files": [target_py],
    }
    res = plan(payload)

    # Verify structured sequence includes contracts
    topo = res.structured_plan.get("topological_edit_sequence", [])
    assert len(topo) >= 1
    first_step = topo[0]
    assert "contracts" in first_step
    assert "validate_python_patch_in_memory" in first_step["contracts"]
    assert "[EXPORTS:" in first_step["action"]
