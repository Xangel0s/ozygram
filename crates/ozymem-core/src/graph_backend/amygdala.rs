use crate::graph_backend::types::{AmygdalaReflexEntry, GraphBackend};
use anyhow::Result;
use rusqlite::params;

/// Normalizes relative file paths across Windows (`\`) and POSIX (`/`)
/// to ensure deterministic database index matches.
pub fn normalize_rel_path(path: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while normalized.starts_with("./") {
        normalized = normalized[2..].to_string();
    }
    normalized
}

impl GraphBackend {
    /// Records or updates an Amygdala pain reflex scar.
    /// Uses an idempotent UPSERT formula: increments occurrences and bumps threat_score safely.
    pub fn record_amygdala_reflex(&self, entry: &AmygdalaReflexEntry) -> Result<()> {
        let norm_path = normalize_rel_path(&entry.file_path);
        let norm_symbol = entry.symbol_name.trim().to_string();
        let reflex_id = if entry.id.trim().is_empty() {
            format!("{}::{}::{}", self.tenant_id, norm_path, norm_symbol)
        } else {
            entry.id.trim().to_string()
        };

        let now = if entry.last_triggered.trim().is_empty() {
            crate::graph_backend::helpers::now_ts()
        } else {
            entry.last_triggered.clone()
        };

        let created_at = if entry.created_at.trim().is_empty() {
            now.clone()
        } else {
            entry.created_at.clone()
        };

        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO amygdala_reflexes (
                id, file_path, symbol_name, threat_score, pain_trigger,
                antidote_directive, occurrences, last_triggered, created_at,
                tenant_id, workspace_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(id) DO UPDATE SET
                occurrences = amygdala_reflexes.occurrences + 1,
                threat_score = MIN(1.0, MAX(amygdala_reflexes.threat_score, excluded.threat_score) + 0.05),
                pain_trigger = excluded.pain_trigger,
                antidote_directive = excluded.antidote_directive,
                last_triggered = excluded.last_triggered",
            params![
                reflex_id,
                norm_path,
                norm_symbol,
                entry.threat_score.clamp(0.0, 1.0),
                entry.pain_trigger.trim(),
                entry.antidote_directive.trim(),
                entry.occurrences.max(1),
                now,
                created_at,
                self.tenant_id,
                inner.workspace_root
            ],
        )?;

        Ok(())
    }

    /// Retrieves all recorded Amygdala reflexes for a given file path,
    /// ordered with the highest threat score and occurrences first.
    pub fn get_amygdala_reflexes_for_file(&self, path: &str) -> Result<Vec<AmygdalaReflexEntry>> {
        let norm_path = normalize_rel_path(path);
        let inner = self.inner.lock().unwrap();

        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, threat_score, pain_trigger,
                    antidote_directive, occurrences, last_triggered, created_at,
                    tenant_id, workspace_root
             FROM amygdala_reflexes
             WHERE file_path = ?1 AND tenant_id = ?2
             ORDER BY threat_score DESC, occurrences DESC",
        )?;

        let rows = stmt.query_map(params![norm_path, self.tenant_id], |row| {
            Ok(AmygdalaReflexEntry {
                id: row.get(0)?,
                file_path: row.get(1)?,
                symbol_name: row.get(2)?,
                threat_score: row.get(3)?,
                pain_trigger: row.get(4)?,
                antidote_directive: row.get(5)?,
                occurrences: row.get(6)?,
                last_triggered: row.get(7)?,
                created_at: row.get(8)?,
                tenant_id: row.get(9)?,
                workspace_root: row.get(10)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Lists all active Amygdala reflexes across the workspace meeting a minimum threat score threshold.
    pub fn list_active_reflexes(&self, min_threat: f64) -> Result<Vec<AmygdalaReflexEntry>> {
        let inner = self.inner.lock().unwrap();

        let mut stmt = inner.sqlite.prepare(
            "SELECT id, file_path, symbol_name, threat_score, pain_trigger,
                    antidote_directive, occurrences, last_triggered, created_at,
                    tenant_id, workspace_root
             FROM amygdala_reflexes
             WHERE threat_score >= ?1 AND tenant_id = ?2
             ORDER BY threat_score DESC, occurrences DESC",
        )?;

        let rows = stmt.query_map(params![min_threat, self.tenant_id], |row| {
            Ok(AmygdalaReflexEntry {
                id: row.get(0)?,
                file_path: row.get(1)?,
                symbol_name: row.get(2)?,
                threat_score: row.get(3)?,
                pain_trigger: row.get(4)?,
                antidote_directive: row.get(5)?,
                occurrences: row.get(6)?,
                last_triggered: row.get(7)?,
                created_at: row.get(8)?,
                tenant_id: row.get(9)?,
                workspace_root: row.get(10)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Deletes an Amygdala reflex by ID.
    pub fn delete_amygdala_reflex(&self, id: &str) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "DELETE FROM amygdala_reflexes WHERE id = ?1 AND tenant_id = ?2",
            params![id, self.tenant_id],
        )?;
        Ok(affected > 0)
    }
}
