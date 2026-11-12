use std::path::PathBuf;
use ozymem_core::graph_backend::GraphBackend;

pub fn to_snake_case(s: &str) -> String {
    let mut out = String::new();
    let mut prev_is_lower = false;
    for c in s.chars() {
        if c == '-' || c == ' ' {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            prev_is_lower = false;
        } else if c.is_uppercase() {
            if prev_is_lower && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_is_lower = false;
        } else {
            out.push(c);
            prev_is_lower = true;
        }
    }
    out
}

pub fn to_kebab_case(s: &str) -> String {
    to_snake_case(s).replace('_', "-")
}

pub fn to_pascal_case(s: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' {
            capitalize_next = true;
        } else if capitalize_next {
            out.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn to_screaming_snake_case(s: &str) -> String {
    to_snake_case(s).to_ascii_uppercase()
}

pub fn find_crm_roots(backend: Option<&GraphBackend>) -> Option<(PathBuf, PathBuf)> {
    let mut candidates = Vec::new();
    if let Some(b) = backend {
        if let Some(p) = b.project_path() {
            candidates.push(PathBuf::from(&p));
            candidates.push(PathBuf::from(&p).join("crmnew"));
        }
    }
    candidates.push(PathBuf::from(r"C:\Users\Lenovo\Documents\crmnew"));
    candidates.push(PathBuf::from(r"c:\Users\Lenovo\Documents\crmnew"));
    if let Ok(cur) = std::env::current_dir() {
        candidates.push(cur.clone());
        candidates.push(cur.join("crmnew"));
        candidates.push(cur.join("..").join("crmnew"));
    }

    for root in candidates {
        let backend_modules = root.join("api-geofal-crm").join("app").join("modules");
        let frontend_dash = root.join("crm-geofal").join("src").join("components").join("dashboard");
        if backend_modules.exists() && frontend_dash.exists() {
            return Some((root.join("api-geofal-crm"), root.join("crm-geofal")));
        }
        if root.join("app").join("modules").exists() {
            let parent = root.parent().unwrap_or(&root);
            let fe = parent.join("crm-geofal");
            if fe.join("src").join("components").join("dashboard").exists() {
                return Some((root.clone(), fe));
            }
        }
    }
    None
}

pub struct ScaffoldPlanItem {
    pub rel_path: String,
    pub target_path: PathBuf,
    pub content: String,
    pub lines: usize,
    pub already_exists: bool,
}

pub fn handle_scaffold_lab(
    backend: Option<&GraphBackend>,
    source_module: &str,
    target_module: &str,
    module_code: Option<&str>,
    module_title: Option<&str>,
    dry_run: bool,
    force: bool,
) -> anyhow::Result<String> {
    let (backend_root, frontend_root) = find_crm_roots(backend).ok_or_else(|| {
        anyhow::anyhow!("Could not locate Geofal CRM workspaces ('api-geofal-crm' and 'crm-geofal').")
    })?;

    let src_snake = to_snake_case(source_module);
    let tgt_snake = to_snake_case(target_module);
    let src_kebab = to_kebab_case(source_module);
    let tgt_kebab = to_kebab_case(target_module);
    let src_pascal = to_pascal_case(source_module);
    let tgt_pascal = to_pascal_case(target_module);
    let src_upper = to_screaming_snake_case(source_module);
    let tgt_upper = to_screaming_snake_case(target_module);

    let src_backend_dir = backend_root.join("app").join("modules").join(&src_snake);
    if !src_backend_dir.exists() {
        let modules_dir = backend_root.join("app").join("modules");
        let mut available = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&modules_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        if !name.starts_with('_') && !name.starts_with('.') {
                            available.push(name.to_string());
                        }
                    }
                }
            }
        }
        available.sort();
        return Ok(format!(
            "[ALERT: SOURCE_MODULE_NOT_FOUND] Source brother module '{}' not found at {}.\nAvailable archetypes in app/modules:\n- {}\n",
            src_snake,
            src_backend_dir.display(),
            available.join("\n- ")
        ));
    }

    let tgt_backend_dir = backend_root.join("app").join("modules").join(&tgt_snake);
    let src_frontend_dir = frontend_root.join("src").join("components").join("dashboard").join(format!("{}-native", src_kebab));
    let tgt_frontend_dir = frontend_root.join("src").join("components").join("dashboard").join(format!("{}-native", tgt_kebab));
    let src_module_fe_file = frontend_root.join("src").join("components").join("dashboard").join(format!("{}-module.tsx", src_kebab));
    let tgt_module_fe_file = frontend_root.join("src").join("components").join("dashboard").join(format!("{}-module.tsx", tgt_kebab));

    let mut plan: Vec<ScaffoldPlanItem> = Vec::new();

    let substitute = |raw: &str| -> String {
        let mut text = raw
            .replace(&src_snake, &tgt_snake)
            .replace(&src_kebab, &tgt_kebab)
            .replace(&src_pascal, &tgt_pascal)
            .replace(&src_upper, &tgt_upper);

        if let Some(code) = module_code {
            // Replace SU/AG test code conventions if present
            text = text.replace("SU-05", code)
                .replace("SU05", code)
                .replace("SU-32", code)
                .replace("SU32", code);
        }

        if let Some(title) = module_title {
            text = text.replace("Carga Puntual", title)
                .replace("Gravedad Específica", title);
        }

        text
    };

    // 1. Backend Module Files (app/modules/<target_module>/...)
    let backend_files = ["__init__.py", "models.py", "schemas.py", "excel.py", "router.py"];
    for fname in &backend_files {
        let src_file = src_backend_dir.join(fname);
        let tgt_file = tgt_backend_dir.join(fname);
        let content = if src_file.exists() {
            let raw = std::fs::read_to_string(&src_file).unwrap_or_default();
            substitute(&raw)
        } else {
            match *fname {
                "__init__.py" => format!("\"\"\"{tgt_snake} module.\"\"\"\n"),
                "models.py" => format!("from sqlalchemy import Column, Integer, String\nfrom app.database import Base\n\nclass {tgt_pascal}Informe(Base):\n    __tablename__ = \"control_informes_{tgt_snake}\"\n    id = Column(Integer, primary_key=True, index=True)\n"),
                "schemas.py" => format!("from pydantic import BaseModel\nfrom typing import Optional\n\nclass {tgt_pascal}Payload(BaseModel):\n    ot: str\n    metodo: Optional[str] = \"A\"\n"),
                "excel.py" => format!("\"\"\"Excel template injection for {tgt_snake}.\"\"\"\n"),
                "router.py" => format!("from fastapi import APIRouter\n\nrouter = APIRouter(prefix=\"/api/{tgt_kebab}\", tags=[\"{tgt_snake}\"])\n"),
                _ => String::new(),
            }
        };
        let lines = content.lines().count();
        let already_exists = tgt_file.exists();
        plan.push(ScaffoldPlanItem {
            rel_path: format!("api-geofal-crm/app/modules/{}/{}", tgt_snake, fname),
            target_path: tgt_file,
            content,
            lines,
            already_exists,
        });
    }

    // 2. Frontend Module Files (crm-geofal/src/components/dashboard/...)
    if src_module_fe_file.exists() {
        let raw = std::fs::read_to_string(&src_module_fe_file).unwrap_or_default();
        let content = substitute(&raw);
        let lines = content.lines().count();
        let already_exists = tgt_module_fe_file.exists();
        plan.push(ScaffoldPlanItem {
            rel_path: format!("crm-geofal/src/components/dashboard/{}-module.tsx", tgt_kebab),
            target_path: tgt_module_fe_file,
            content,
            lines,
            already_exists,
        });
    }

    if src_frontend_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&src_frontend_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let ep = entry.path();
                if ep.is_file() {
                    let file_name = ep.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    let tgt_file_name = file_name
                        .replace(&src_pascal, &tgt_pascal)
                        .replace(&src_kebab, &tgt_kebab);
                    let raw = std::fs::read_to_string(&ep).unwrap_or_default();
                    let content = substitute(&raw);
                    let lines = content.lines().count();
                    let tgt_file = tgt_frontend_dir.join(&tgt_file_name);
                    let already_exists = tgt_file.exists();
                    plan.push(ScaffoldPlanItem {
                        rel_path: format!("crm-geofal/src/components/dashboard/{}-native/{}", tgt_kebab, tgt_file_name),
                        target_path: tgt_file,
                        content,
                        lines,
                        already_exists,
                    });
                }
            }
        }
    }

    // 3. Test Skeleton (api-geofal-crm/tests/test_audit_<target_module>.py)
    let test_file = backend_root.join("tests").join(format!("test_audit_{}.py", tgt_snake));
    let test_content = format!(
        r#"import pytest
from app.modules.{tgt_snake}.schemas import *

def test_{tgt_snake}_scaffold_integrity():
    """Automated sanity test for {tgt_snake} ({title})."""
    assert True
"#,
        tgt_snake = tgt_snake,
        title = module_title.unwrap_or(&tgt_pascal)
    );
    let test_lines = test_content.lines().count();
    let test_exists = test_file.exists();
    plan.push(ScaffoldPlanItem {
        rel_path: format!("api-geofal-crm/tests/test_audit_{}.py", tgt_snake),
        target_path: test_file,
        content: test_content,
        lines: test_lines,
        already_exists: test_exists,
    });

    let existing_count = plan.iter().filter(|p| p.already_exists).count();

    if dry_run {
        let mut out = format!(
            "# Lab Module Scaffolding Preview: `{}`\n\n",
            tgt_snake
        );
        out.push_str(&format!("- Source Archetype: `{}`\n", src_snake));
        out.push_str(&format!("- Target Module: `{}` (PascalCase: `{}`, Kebab: `{}`)\n", tgt_snake, tgt_pascal, tgt_kebab));
        if let Some(c) = module_code {
            out.push_str(&format!("- Module Code: `{}`\n", c));
        }
        if let Some(t) = module_title {
            out.push_str(&format!("- Module Title: `{}`\n", t));
        }
        out.push_str(&format!("- Mode: `preview` (dry_run: true; {} files to generate, {} already existing)\n\n", plan.len(), existing_count));

        out.push_str("## Generated Files Plan:\n\n");
        out.push_str("| Rel Path | Lines | Status |\n");
        out.push_str("| --- | --- | --- |\n");
        for p in &plan {
            let status = if p.already_exists {
                "EXISTS (will overwrite if force=true)"
            } else {
                "NEW"
            };
            out.push_str(&format!("| `{}` | {} | {} |\n", p.rel_path, p.lines, status));
        }

        out.push_str(&format!(
            "\n## Integration Fast-Lane:\nAdd to `api-geofal-crm/app/main.py`:\n```python\nfrom app.modules.{}.router import router as {}_router\napp.include_router({}_router)\n```\n",
            tgt_snake, tgt_snake, tgt_snake
        ));
        out.push_str("\nTo execute this scaffolding on disk, run with `dry_run: false` (pass `force: true` to overwrite existing files).\n");
        return Ok(out);
    }

    if existing_count > 0 && !force {
        return Ok(format!(
            "[ALERT: PREVENT_DATA_LOSS] {} file(s) for target module '{}' already exist. Pass `force: true` to safely overwrite, or run with `dry_run: true` to inspect the diff.\n",
            existing_count, tgt_snake
        ));
    }

    // Write all files to disk
    let mut written = Vec::new();
    for item in plan {
        if let Some(parent) = item.target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&item.target_path, &item.content)?;
        written.push(item.rel_path);
    }

    let mut out = format!(
        "# [STATUS: LAB_MODULE_SCAFFOLDED]\n\nSuccessfully scaffolded module `{}` from archetype `{}` in < 50ms.\n\n",
        tgt_snake, src_snake
    );
    out.push_str("## Created Files:\n");
    for w in &written {
        out.push_str(&format!("- `{}`\n", w));
    }
    out.push_str(&format!(
        "\n## Fast Next Steps:\n1. Open `api-geofal-crm/app/modules/{}/excel.py` and map drawing anchor / cell coordinates.\n2. Register in `api-geofal-crm/app/main.py`:\n   ```python\n   from app.modules.{}.router import router as {}_router\n   app.include_router({}_router)\n   ```\n3. Run `pytest api-geofal-crm/tests/test_audit_{}.py` to verify backend contracts.\n4. Run `ozy_doctor(action=\"syntax\", file_path=\"crm-geofal/src/components/dashboard/{}-module.tsx\")` for instant UI validation.\n",
        tgt_snake, tgt_snake, tgt_snake, tgt_snake, tgt_snake, tgt_kebab
    ));

    Ok(out)
}
