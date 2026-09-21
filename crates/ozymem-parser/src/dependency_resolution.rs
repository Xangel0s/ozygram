use std::fs;
use std::path::{Path, PathBuf};

use crate::{DependencyHintKind, ParsedDependencyHint};

const INTERNAL_CRATE_NAMES: &[&str] = &[
    "ozymem_core",
    "ozymem_parser",
    "ozymem_cli",
    "ozymem_server",
];

const PYTHON_EXTERNAL_MODULES: &[&str] = &[
    "os", "sys", "time", "datetime", "json", "math", "re", "logging", "typing", "collections",
    "itertools", "functools", "pathlib", "shutil", "subprocess", "unittest", "pytest", "abc",
    "copy", "io", "enum", "dataclasses", "uuid", "hashlib", "base64", "urllib", "http",
    "asyncio", "socket", "threading", "multiprocessing", "queue", "tempfile", "glob",
    "fastapi", "pydantic", "sqlalchemy", "starlette", "uvicorn", "alembic", "requests", "httpx",
    "aiohttp", "flask", "django", "numpy", "pandas", "scipy", "dotenv", "jose", "passlib",
    "boto3", "jwt", "yaml", "click", "typer", "celery", "redis", "psycopg2", "asyncpg",
];

pub fn is_internal_dependency_hint(hint: &ParsedDependencyHint) -> bool {
    match hint.kind {
        DependencyHintKind::ModItem => true,
        DependencyHintKind::UseDeclaration => {
            let root = dependency_root_segment(&hint.label);
            matches!(root.as_deref(), Some("crate" | "self" | "super"))
                || root
                    .as_deref()
                    .is_some_and(|root| INTERNAL_CRATE_NAMES.contains(&root))
        }
        DependencyHintKind::ExportStatement => {
            hint.label.starts_with('.')
                || hint.label.starts_with('/')
                || hint.label.starts_with("@/")
                || hint.label.starts_with("~/")
        }
        DependencyHintKind::ImportStatement => {
            if hint.label.starts_with('.')
                || hint.label.starts_with('/')
                || hint.label.starts_with("@/")
                || hint.label.starts_with("~/")
            {
                return true;
            }

            let path = Path::new(&hint.file_path);
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            // JS/TS non-relative and non-aliased imports (e.g. 'react', 'lodash', 'express') are external npm packages
            if matches!(ext.as_str(), "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs") {
                return false;
            }

            let first_segment = hint
                .label
                .split('.')
                .next()
                .unwrap_or(&hint.label);

            !PYTHON_EXTERNAL_MODULES.contains(&first_segment)
        }
    }
}

pub fn resolve_dependency_target(
    hint: &ParsedDependencyHint,
    current_file_path: impl AsRef<Path>,
) -> Option<PathBuf> {
    if !is_internal_dependency_hint(hint) {
        return None;
    }

    let current_file = normalize_absolute_path(current_file_path.as_ref())?;
    let current_dir = current_file.parent()?;

    match hint.kind {
        DependencyHintKind::ModItem => resolve_mod_item_target(&hint.label, current_dir),
        DependencyHintKind::UseDeclaration => {
            resolve_use_target(&hint.label, &current_file, current_dir)
        }
        DependencyHintKind::ImportStatement | DependencyHintKind::ExportStatement => {
            let ext = current_file
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if ext == "py" {
                resolve_python_target(&hint.label, &current_file, current_dir)
            } else if matches!(ext.as_str(), "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs") {
                resolve_js_ts_target(&hint.label, &current_file, current_dir)
            } else {
                resolve_python_target(&hint.label, &current_file, current_dir)
                    .or_else(|| resolve_js_ts_target(&hint.label, &current_file, current_dir))
            }
        }
    }
}

fn resolve_python_target(
    label: &str,
    current_file: &Path,
    current_dir: &Path,
) -> Option<PathBuf> {
    let clean_label = label.trim();
    if clean_label.is_empty() {
        return None;
    }

    if clean_label.starts_with('.') {
        let dot_count = clean_label.chars().take_while(|c| *c == '.').count();
        let remainder = &clean_label[dot_count..];

        let mut base_dir = current_dir;
        for _ in 1..dot_count {
            base_dir = base_dir.parent()?;
        }

        if remainder.is_empty() {
            return first_existing(&[base_dir.join("__init__.py")]);
        }

        let segments: Vec<&str> = remainder.split('.').filter(|s| !s.is_empty()).collect();
        resolve_python_module_in_dir(base_dir, &segments)
    } else {
        let segments: Vec<&str> = clean_label.split('.').filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            return None;
        }

        if let Some(target) = resolve_python_module_in_dir(current_dir, &segments) {
            return Some(target);
        }

        for ancestor in current_file.ancestors().skip(1) {
            if let Some(target) = resolve_python_module_in_dir(ancestor, &segments) {
                return Some(target);
            }
            if ancestor.join(".git").exists()
                || ancestor.join("pyproject.toml").is_file()
                || ancestor.join("setup.py").is_file()
            {
                break;
            }
        }

        None
    }
}

fn resolve_python_module_in_dir(base_dir: &Path, segments: &[&str]) -> Option<PathBuf> {
    if segments.is_empty() {
        return None;
    }

    for len in (1..=segments.len()).rev() {
        let subpath = segments[..len].join("/");
        let candidates = [
            base_dir.join(format!("{subpath}.py")),
            base_dir.join(&subpath).join("__init__.py"),
        ];
        if let Some(found) = first_existing(&candidates) {
            return Some(found);
        }
    }

    None
}

fn resolve_js_ts_target(
    label: &str,
    current_file: &Path,
    current_dir: &Path,
) -> Option<PathBuf> {
    let clean = label.trim().trim_matches(|c| c == '\'' || c == '"' || c == '`');
    if clean.is_empty() {
        return None;
    }

    let (base_dir, relative_path) = if clean.starts_with("./") || clean.starts_with("../") {
        (current_dir.to_path_buf(), clean.to_string())
    } else if let Some(stripped) = clean.strip_prefix("@/") {
        let root = find_project_root_for_js(current_file);
        let src = root.join("src");
        let base = if src.is_dir() { src } else { root };
        (base, stripped.to_string())
    } else if let Some(stripped) = clean.strip_prefix("~/") {
        let root = find_project_root_for_js(current_file);
        (root, stripped.to_string())
    } else if clean.starts_with('/') {
        let root = find_project_root_for_js(current_file);
        (root, clean.trim_start_matches('/').to_string())
    } else {
        return None;
    };

    let target_base = base_dir.join(relative_path);
    let candidates = [
        target_base.clone(),
        target_base.with_extension("ts"),
        target_base.with_extension("tsx"),
        target_base.with_extension("js"),
        target_base.with_extension("jsx"),
        target_base.join("index.ts"),
        target_base.join("index.tsx"),
        target_base.join("index.js"),
        target_base.join("index.jsx"),
    ];

    first_existing(&candidates)
}

fn find_project_root_for_js(current_file: &Path) -> PathBuf {
    for ancestor in current_file.ancestors().skip(1) {
        if ancestor.join("package.json").is_file()
            || ancestor.join("tsconfig.json").is_file()
            || ancestor.join(".git").exists()
        {
            return ancestor.to_path_buf();
        }
    }
    current_file.parent().unwrap_or(current_file).to_path_buf()
}

fn resolve_mod_item_target(label: &str, current_dir: &Path) -> Option<PathBuf> {
    let module_name = sanitize_identifier(label);
    if module_name.is_empty() {
        return None;
    }

    let candidates = [
        current_dir.join(format!("{module_name}.rs")),
        current_dir.join(&module_name).join("mod.rs"),
    ];

    first_existing(&candidates)
}

fn resolve_use_target(label: &str, current_file: &Path, current_dir: &Path) -> Option<PathBuf> {
    let import = normalize_import_path(label);
    let segments = split_path_segments(&import);
    if segments.is_empty() {
        return None;
    }

    if segments[0] == "crate" {
        return resolve_current_crate_target(&segments[1..], current_file, current_dir);
    }

    if segments[0] == "self" {
        return resolve_relative_module_target(&segments[1..], current_dir);
    }

    if segments[0] == "super" {
        return current_dir
            .parent()
            .and_then(|parent| resolve_relative_module_target(&segments[1..], parent));
    }

    resolve_sibling_crate_target(&segments, current_file)
        .or_else(|| resolve_relative_module_target(&segments, current_dir))
}

fn resolve_current_crate_target(
    segments: &[&str],
    current_file: &Path,
    current_dir: &Path,
) -> Option<PathBuf> {
    let crate_root = find_crate_root(current_file)?;
    let src_root = crate_root.join("src");

    if segments.is_empty() {
        return first_existing(&[src_root.join("lib.rs"), src_root.join("main.rs")]);
    }

    resolve_module_path(&src_root, segments)
        .or_else(|| resolve_relative_module_target(segments, current_dir))
}

fn resolve_relative_module_target(segments: &[&str], base_dir: &Path) -> Option<PathBuf> {
    if segments.is_empty() {
        return None;
    }

    resolve_module_path(base_dir, segments)
}

fn resolve_module_path(base_dir: &Path, segments: &[&str]) -> Option<PathBuf> {
    for prefix_len in (1..=segments.len()).rev() {
        let module_path = segments[..prefix_len].join("/");
        let candidates = [
            base_dir.join(format!("{module_path}.rs")),
            base_dir.join(&module_path).join("mod.rs"),
        ];

        if let Some(found) = first_existing(&candidates) {
            return Some(found);
        }
    }

    None
}

fn resolve_sibling_crate_target(segments: &[&str], current_file: &Path) -> Option<PathBuf> {
    let workspace_root = find_workspace_root(current_file)?;
    let crate_name = segments[0].replace('_', "-");
    let crate_root = workspace_root.join("crates").join(crate_name);

    first_existing(&[
        crate_root.join("src/lib.rs"),
        crate_root.join("src/main.rs"),
    ])
}

fn find_crate_root(current_file: &Path) -> Option<PathBuf> {
    current_file
        .ancestors()
        .skip(1)
        .find(|ancestor| ancestor.join("Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

fn find_workspace_root(current_file: &Path) -> Option<PathBuf> {
    current_file
        .ancestors()
        .find(|ancestor| {
            let cargo_toml = ancestor.join("Cargo.toml");
            cargo_toml.is_file()
                && fs::read_to_string(cargo_toml)
                    .map(|contents| contents.contains("[workspace]"))
                    .unwrap_or(false)
        })
        .map(Path::to_path_buf)
}

fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{}", stripped))
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path
    }
}

fn normalize_absolute_path(path: &Path) -> Option<PathBuf> {
    let p = if path.is_absolute() {
        // Use canonicalize so resolved paths are consistent with paths
        // stored by full_scan (which also canonicalizes).  Fall back to the
        // raw path when the file doesn't exist yet (e.g. resolution of a
        // not-yet-created file during a live-coding session).
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        let absolute = std::env::current_dir().ok()?.join(path);
        fs::canonicalize(&absolute).unwrap_or(absolute)
    };
    Some(strip_unc_prefix(p))
}

fn normalize_import_path(label: &str) -> String {
    let before_alias = label
        .split_once(" as ")
        .map(|(path, _)| path)
        .unwrap_or(label);
    let before_group = before_alias.split("::{").next().unwrap_or(before_alias);
    let before_wildcard = before_group.split("::*").next().unwrap_or(before_group);

    before_wildcard.trim().trim_end_matches(';').to_string()
}

fn dependency_root_segment(label: &str) -> Option<String> {
    let normalized = normalize_import_path(label);
    normalized
        .split("::")
        .find(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
}

fn split_path_segments(path: &str) -> Vec<&str> {
    path.split("::")
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn sanitize_identifier(value: &str) -> String {
    value
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == '$'
        })
        .collect()
}

fn first_existing(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .map(|p| strip_unc_prefix(fs::canonicalize(p).unwrap_or_else(|_| p.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, "// test").expect("write file");
    }

    /// Canonicalize an expected path, stripping UNC prefix.
    /// This ensures tests pass on macOS where /tmp is a symlink to /private/tmp.
    fn canon(path: PathBuf) -> PathBuf {
        strip_unc_prefix(fs::canonicalize(&path).unwrap_or(path))
    }

    #[test]
    fn resolves_mod_item_in_same_directory() {
        let root = std::env::temp_dir().join(format!("ozymem-resolve-mod-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        create_file(&root.join("Cargo.toml"));
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        create_file(&root.join("src/lib.rs"));
        create_file(&root.join("src/router.rs"));

        let hint = ParsedDependencyHint {
            file_path: root.join("src/lib.rs").to_string_lossy().to_string(),
            kind: DependencyHintKind::ModItem,
            label: "router".to_string(),
            raw_text: "mod router;".to_string(),
            start_line: 1,
            end_line: 1,
        };

        let resolved = resolve_dependency_target(&hint, root.join("src/lib.rs"))
            .expect("should resolve module");

        assert_eq!(
            resolved,
            canon(root.join("src/router.rs"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolves_current_crate_use_to_module_file() {
        let root = std::env::temp_dir().join(format!("ozymem-resolve-use-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        create_file(&root.join("Cargo.toml"));
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write cargo");
        create_file(&root.join("src/lib.rs"));
        create_file(&root.join("src/domain.rs"));

        let hint = ParsedDependencyHint {
            file_path: root.join("src/lib.rs").to_string_lossy().to_string(),
            kind: DependencyHintKind::UseDeclaration,
            label: "crate::domain::User".to_string(),
            raw_text: "use crate::domain::User;".to_string(),
            start_line: 1,
            end_line: 1,
        };

        let resolved = resolve_dependency_target(&hint, root.join("src/lib.rs"))
            .expect("should resolve crate use");

        assert_eq!(
            resolved,
            canon(root.join("src/domain.rs"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolves_sibling_crate_to_root_file() {
        let root =
            std::env::temp_dir().join(format!("ozymem-resolve-sibling-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        create_file(&root.join("Cargo.toml"));
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/ozymem-core\", \"crates/ozymem-parser\"]\n",
        )
        .expect("write workspace cargo");
        create_file(&root.join("crates/ozymem-parser/Cargo.toml"));
        create_file(&root.join("crates/ozymem-parser/src/lib.rs"));
        create_file(&root.join("crates/ozymem-core/Cargo.toml"));
        create_file(&root.join("crates/ozymem-core/src/lib.rs"));

        let hint = ParsedDependencyHint {
            file_path: root
                .join("crates/ozymem-parser/src/lib.rs")
                .to_string_lossy()
                .to_string(),
            kind: DependencyHintKind::UseDeclaration,
            label: "ozymem_core::GraphBackend".to_string(),
            raw_text: "use ozymem_core::GraphBackend;".to_string(),
            start_line: 1,
            end_line: 1,
        };

        let resolved =
            resolve_dependency_target(&hint, root.join("crates/ozymem-parser/src/lib.rs"))
                .expect("should resolve sibling crate");

        assert_eq!(
            resolved,
            canon(root.join("crates/ozymem-core/src/lib.rs"))
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn filters_external_dependencies() {
        let hint_rust = ParsedDependencyHint {
            file_path: "/tmp/src/lib.rs".to_string(),
            kind: DependencyHintKind::UseDeclaration,
            label: "serde::Serialize".to_string(),
            raw_text: "use serde::Serialize;".to_string(),
            start_line: 1,
            end_line: 1,
        };
        assert!(!is_internal_dependency_hint(&hint_rust));
        assert!(resolve_dependency_target(&hint_rust, "/tmp/src/lib.rs").is_none());

        let hint_py = ParsedDependencyHint {
            file_path: "/tmp/app.py".to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "fastapi".to_string(),
            raw_text: "import fastapi".to_string(),
            start_line: 1,
            end_line: 1,
        };
        assert!(!is_internal_dependency_hint(&hint_py));

        let hint_js = ParsedDependencyHint {
            file_path: "/tmp/index.ts".to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "react".to_string(),
            raw_text: "import React from 'react'".to_string(),
            start_line: 1,
            end_line: 1,
        };
        assert!(!is_internal_dependency_hint(&hint_js));
    }

    #[test]
    fn resolves_python_relative_and_absolute_imports() {
        let root = std::env::temp_dir().join(format!("ozymem-resolve-py-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        create_file(&root.join("pyproject.toml"));
        create_file(&root.join("app/__init__.py"));
        create_file(&root.join("app/routers/__init__.py"));
        create_file(&root.join("app/routers/user.py"));
        create_file(&root.join("app/models.py"));
        create_file(&root.join("app/services/auth.py"));

        let user_py = root.join("app/routers/user.py");

        // Relative import: from ..models import User
        let hint_rel = ParsedDependencyHint {
            file_path: user_py.to_string_lossy().to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "..models".to_string(),
            raw_text: "from ..models import User".to_string(),
            start_line: 1,
            end_line: 1,
        };
        let resolved = resolve_dependency_target(&hint_rel, &user_py).expect("should resolve relative models");
        assert_eq!(resolved, canon(root.join("app/models.py")));

        // Nested relative import: from ..services.auth import verify
        let hint_auth = ParsedDependencyHint {
            file_path: user_py.to_string_lossy().to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "..services.auth".to_string(),
            raw_text: "from ..services.auth import verify".to_string(),
            start_line: 2,
            end_line: 2,
        };
        let resolved_auth = resolve_dependency_target(&hint_auth, &user_py).expect("should resolve auth");
        assert_eq!(resolved_auth, canon(root.join("app/services/auth.py")));

        // Absolute project import: from app.models import User
        let hint_abs = ParsedDependencyHint {
            file_path: user_py.to_string_lossy().to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "app.models".to_string(),
            raw_text: "from app.models import User".to_string(),
            start_line: 3,
            end_line: 3,
        };
        let resolved_abs = resolve_dependency_target(&hint_abs, &user_py).expect("should resolve abs models");
        assert_eq!(resolved_abs, canon(root.join("app/models.py")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolves_js_ts_relative_and_aliased_imports() {
        let root = std::env::temp_dir().join(format!("ozymem-resolve-js-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        create_file(&root.join("package.json"));
        create_file(&root.join("src/components/Button.tsx"));
        create_file(&root.join("src/components/Header.tsx"));
        create_file(&root.join("src/utils/index.ts"));

        let header_tsx = root.join("src/components/Header.tsx");

        // Relative import: import { Button } from './Button';
        let hint_rel = ParsedDependencyHint {
            file_path: header_tsx.to_string_lossy().to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "./Button".to_string(),
            raw_text: "import { Button } from './Button'".to_string(),
            start_line: 1,
            end_line: 1,
        };
        let resolved_rel = resolve_dependency_target(&hint_rel, &header_tsx).expect("should resolve Button.tsx");
        assert_eq!(resolved_rel, canon(root.join("src/components/Button.tsx")));

        // Directory index import: import { format } from '../utils';
        let hint_index = ParsedDependencyHint {
            file_path: header_tsx.to_string_lossy().to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "../utils".to_string(),
            raw_text: "import { format } from '../utils'".to_string(),
            start_line: 2,
            end_line: 2,
        };
        let resolved_index = resolve_dependency_target(&hint_index, &header_tsx).expect("should resolve index.ts");
        assert_eq!(resolved_index, canon(root.join("src/utils/index.ts")));

        // Aliased import: import { Button } from '@/components/Button';
        let hint_alias = ParsedDependencyHint {
            file_path: header_tsx.to_string_lossy().to_string(),
            kind: DependencyHintKind::ImportStatement,
            label: "@/components/Button".to_string(),
            raw_text: "import { Button } from '@/components/Button'".to_string(),
            start_line: 3,
            end_line: 3,
        };
        let resolved_alias = resolve_dependency_target(&hint_alias, &header_tsx).expect("should resolve alias Button.tsx");
        assert_eq!(resolved_alias, canon(root.join("src/components/Button.tsx")));

        let _ = fs::remove_dir_all(&root);
    }
}
