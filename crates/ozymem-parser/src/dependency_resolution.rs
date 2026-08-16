use std::fs;
use std::path::{Path, PathBuf};

use crate::{DependencyHintKind, ParsedDependencyHint};

const INTERNAL_CRATE_NAMES: &[&str] = &[
    "ozymem_core",
    "ozymem_parser",
    "ozymem_cli",
    "ozymem_server",
];

pub fn is_internal_dependency_hint(hint: &ParsedDependencyHint) -> bool {
    if matches!(hint.kind, DependencyHintKind::ModItem) {
        return true;
    }

    let root = dependency_root_segment(&hint.label);
    matches!(root.as_deref(), Some("crate" | "self" | "super"))
        || root
            .as_deref()
            .is_some_and(|root| INTERNAL_CRATE_NAMES.contains(&root))
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
    }
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
        .map(|p| strip_unc_prefix(p.clone()))
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
        let hint = ParsedDependencyHint {
            file_path: "/tmp/src/lib.rs".to_string(),
            kind: DependencyHintKind::UseDeclaration,
            label: "serde::Serialize".to_string(),
            raw_text: "use serde::Serialize;".to_string(),
            start_line: 1,
            end_line: 1,
        };

        assert!(!is_internal_dependency_hint(&hint));
        assert!(resolve_dependency_target(&hint, "/tmp/src/lib.rs").is_none());
    }
}
