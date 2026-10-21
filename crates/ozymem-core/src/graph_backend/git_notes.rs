use anyhow::Result;
use crate::graph_backend::GraphBackend;
use crate::mcp_common::McpBackend;

impl GraphBackend {
    /// Exporta el estado de memoria actual (lecciones, contratos engram, reglas procedimentales)
    /// hacia una nota en `refs/notes/ozymem` vinculada al commit actual de Git.
    pub async fn export_to_git_note(
        &self,
        commit_ref: Option<&str>,
        note_ref: Option<&str>,
        procedural_rules: Option<Vec<serde_json::Value>>,
    ) -> Result<(String, crate::OzymemGitNotePayload)> {
        let project_path = self.project_path()
            .ok_or_else(|| anyhow::anyhow!("No project path configured in GraphBackend"))?;
        let git = crate::git_backend::GitBackend::open(std::path::Path::new(&project_path))
            .map_err(|e| anyhow::anyhow!("Git repository not found: {e}"))?;

        let head_commit = git.head_commit_hash().unwrap_or_else(|| "0000000000000000000000000000000000000000".to_string());
        let branch = git.current_branch();

        let lessons = self.recent_lessons(None, 200).await.unwrap_or_default();
        let all_files = self.list_all_files().unwrap_or_default();
        let mut engram_contracts = Vec::new();

        for f in &all_files {
            if let Ok(Some(ctx)) = self.get_file_context(f).await {
                engram_contracts.extend(ctx.engram_contracts);
            }
        }

        let payload = crate::OzymemGitNotePayload::new(
            head_commit,
            branch,
            lessons,
            engram_contracts,
            procedural_rules.unwrap_or_default(),
        );

        let note_text = payload.serialize()?;
        let note_oid = git.write_note(commit_ref, note_ref, &note_text, true)
            .map_err(|e| anyhow::anyhow!("Failed to write git note: {e}"))?;

        Ok((note_oid, payload))
    }

    /// Importa y fusiona el estado de memoria desde una nota en `refs/notes/ozymem`.
    pub async fn import_from_git_note(
        &self,
        commit_ref: Option<&str>,
        note_ref: Option<&str>,
    ) -> Result<crate::OzymemGitNotePayload> {
        let project_path = self.project_path()
            .ok_or_else(|| anyhow::anyhow!("No project path configured in GraphBackend"))?;
        let git = crate::git_backend::GitBackend::open(std::path::Path::new(&project_path))
            .map_err(|e| anyhow::anyhow!("Git repository not found: {e}"))?;

        let note_text = git.read_note(commit_ref, note_ref)
            .map_err(|e| anyhow::anyhow!("Failed to read git note: {e}"))?
            .ok_or_else(|| anyhow::anyhow!("No git note found at specified commit/ref"))?;

        let payload = crate::OzymemGitNotePayload::deserialize(&note_text)?;

        // 1. Fusionar lecciones
        for l in &payload.lessons {
            let _ = self.record_entry(
                &l.file_path,
                Some(&l.symbol_name),
                &l.error_context,
                &l.solution,
                &l.kind,
            ).await;
        }

        // 2. Fusionar contratos engram en IncrementalEngramStore
        for c in &payload.engram_contracts {
            self.engram_store.insert(c.clone());
        }

        Ok(payload)
    }

    /// Lista todas las notas de memoria registradas en `refs/notes/ozymem`.
    pub fn list_all_git_notes(
        &self,
        note_ref: Option<&str>,
    ) -> Result<Vec<(String, crate::OzymemGitNotePayload)>> {
        let project_path = self.project_path()
            .ok_or_else(|| anyhow::anyhow!("No project path configured in GraphBackend"))?;
        let git = crate::git_backend::GitBackend::open(std::path::Path::new(&project_path))
            .map_err(|e| anyhow::anyhow!("Git repository not found: {e}"))?;

        let raw_notes = git.list_notes(note_ref)
            .map_err(|e| anyhow::anyhow!("Failed to list git notes: {e}"))?;

        let mut results = Vec::new();
        for (commit, text) in raw_notes {
            if let Ok(payload) = crate::OzymemGitNotePayload::deserialize(&text) {
                results.push((commit, payload));
            }
        }

        Ok(results)
    }

    /// Lee las heurísticas de equipo en formato JSONL desde `refs/notes/ozy-heuristics`.
    pub fn read_federated_heuristics(
        &self,
        commit_ref: Option<&str>,
        note_ref: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let project_path = self.project_path()
            .ok_or_else(|| anyhow::anyhow!("No project path configured in GraphBackend"))?;
        let git = crate::git_backend::GitBackend::open(std::path::Path::new(&project_path))
            .map_err(|e| anyhow::anyhow!("Git repository not found: {e}"))?;

        let target_ref = note_ref.unwrap_or(OZY_HEURISTICS_NOTE_REF);
        let note_text = match git.read_note(commit_ref, Some(target_ref)) {
            Ok(Some(t)) => t,
            _ => return Ok(Vec::new()),
        };

        let mut heuristics = Vec::new();
        for line in note_text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                heuristics.push(val);
            }
        }
        Ok(heuristics)
    }

    /// Escribe una lista de heurísticas como JSONL en `refs/notes/ozy-heuristics`.
    pub fn write_federated_heuristics(
        &self,
        commit_ref: Option<&str>,
        note_ref: Option<&str>,
        heuristics: &[serde_json::Value],
        force: bool,
    ) -> Result<String> {
        let project_path = self.project_path()
            .ok_or_else(|| anyhow::anyhow!("No project path configured in GraphBackend"))?;
        let git = crate::git_backend::GitBackend::open(std::path::Path::new(&project_path))
            .map_err(|e| anyhow::anyhow!("Git repository not found: {e}"))?;

        let target_ref = note_ref.unwrap_or(OZY_HEURISTICS_NOTE_REF);
        let mut lines = Vec::new();
        for h in heuristics {
            lines.push(serde_json::to_string(h)?);
        }
        lines.sort();
        let payload = lines.join("\n");

        git.write_note(commit_ref, Some(target_ref), &payload, force)
            .map_err(|e| anyhow::anyhow!("Failed to write federated heuristics note: {e}"))
    }
}

pub const OZY_HEURISTICS_NOTE_REF: &str = "refs/notes/ozy-heuristics";

