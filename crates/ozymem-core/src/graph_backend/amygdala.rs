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
                tenant_id, workspace_root, clean_runs, extinction_score
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                occurrences = amygdala_reflexes.occurrences + 1,
                threat_score = MIN(1.0, MAX(amygdala_reflexes.threat_score, excluded.threat_score) + 0.05),
                pain_trigger = excluded.pain_trigger,
                antidote_directive = excluded.antidote_directive,
                last_triggered = excluded.last_triggered,
                clean_runs = 0,
                extinction_score = 0.0",
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
                inner.workspace_root,
                entry.clean_runs.max(0),
                entry.extinction_score.clamp(0.0, 1.0),
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
                    tenant_id, workspace_root, COALESCE(clean_runs, 0), COALESCE(extinction_score, 0.0)
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
                clean_runs: row.get(11)?,
                extinction_score: row.get(12)?,
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
                    tenant_id, workspace_root, COALESCE(clean_runs, 0), COALESCE(extinction_score, 0.0)
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
                clean_runs: row.get(11)?,
                extinction_score: row.get(12)?,
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

    /// Records a clean, successful validation run on a file, triggering vmPFC extinction learning.
    /// Dampens the effective threat without erasing the historical memory (prevents artificial PTSD).
    pub fn record_amygdala_clean_run(&self, path: &str) -> Result<f64> {
        let norm_path = normalize_rel_path(path);
        let inner = self.inner.lock().unwrap();

        let current_runs: i64 = inner.sqlite.query_row(
            "SELECT COALESCE(MAX(clean_runs), 0) FROM amygdala_reflexes WHERE file_path = ?1 AND tenant_id = ?2",
            params![norm_path, self.tenant_id],
            |r| r.get(0),
        ).unwrap_or(0);

        let new_runs = current_runs + 1;
        let k = 0.8_f64;
        let theta = 3.0_f64;
        let exp_val = (-k * (new_runs as f64 - theta)).exp();
        let extinction_score = (1.0 / (1.0 + exp_val)).clamp(0.0, 1.0);

        inner.sqlite.execute(
            "UPDATE amygdala_reflexes
             SET clean_runs = ?1, extinction_score = ?2
             WHERE file_path = ?3 AND tenant_id = ?4",
            params![new_runs, extinction_score, norm_path, self.tenant_id],
        )?;

        Ok(extinction_score)
    }

    /// Records a threat relapse upon a new error, resetting clean_runs and extinction inhibition.
    pub fn record_amygdala_relapse(&self, path: &str) -> Result<()> {
        let norm_path = normalize_rel_path(path);
        let inner = self.inner.lock().unwrap();

        inner.sqlite.execute(
            "UPDATE amygdala_reflexes
             SET clean_runs = 0, extinction_score = 0.0
             WHERE file_path = ?1 AND tenant_id = ?2",
            params![norm_path, self.tenant_id],
        )?;

        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_amygdala_extinction_and_relapse() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_amygdala.db");
        let backend = GraphBackend::open(Some(db_path.to_str().unwrap())).unwrap();

        let entry = AmygdalaReflexEntry {
            id: "".to_string(),
            file_path: "src/critical.rs".to_string(),
            symbol_name: "".to_string(),
            threat_score: 0.90,
            pain_trigger: "crash in compiler".to_string(),
            antidote_directive: "check arity".to_string(),
            occurrences: 3,
            last_triggered: "".to_string(),
            created_at: "".to_string(),
            tenant_id: "local".to_string(),
            workspace_root: "".to_string(),
            clean_runs: 0,
            extinction_score: 0.0,
        };

        // 1. Initial record
        backend.record_amygdala_reflex(&entry).unwrap();
        let reflexes = backend.get_amygdala_reflexes_for_file("src/critical.rs").unwrap();
        assert_eq!(reflexes.len(), 1);
        assert_eq!(reflexes[0].autonomic_response(), "CRITICAL_VETO");
        assert_eq!(reflexes[0].effective_threat(), 0.90);

        // 2. Perform 5 clean runs
        for _ in 0..5 {
            let _ = backend.record_amygdala_clean_run("src/critical.rs").unwrap();
        }

        let post_ext = backend.get_amygdala_reflexes_for_file("src/critical.rs").unwrap();
        assert!(post_ext[0].extinction_score > 0.80);
        assert!(post_ext[0].effective_threat() < 0.20);
        assert_eq!(post_ext[0].autonomic_response(), "EXTINGUISHED_SAFE");

        // 3. Relapse event
        backend.record_amygdala_relapse("src/critical.rs").unwrap();
        let post_relapse = backend.get_amygdala_reflexes_for_file("src/critical.rs").unwrap();
        assert_eq!(post_relapse[0].clean_runs, 0);
        assert_eq!(post_relapse[0].extinction_score, 0.0);
        assert_eq!(post_relapse[0].autonomic_response(), "CRITICAL_VETO");
    }
}
