use crate::graph_backend::amygdala::normalize_rel_path;
use crate::graph_backend::types::{GraphBackend, ProceduralHabitEntry};
use anyhow::Result;
use rusqlite::params;

impl GraphBackend {
    /// Records or updates a procedural habit playbook mined from Git and session co-edits.
    pub fn record_procedural_habit(&self, entry: &ProceduralHabitEntry) -> Result<()> {
        let norm_trigger = normalize_rel_path(&entry.trigger_file);
        let habit_id = if entry.id.trim().is_empty() {
            format!("{}::{}", self.tenant_id, norm_trigger)
        } else {
            entry.id.trim().to_string()
        };

        let normalized_subsequent: Vec<String> = entry
            .subsequent_files
            .iter()
            .map(|f| normalize_rel_path(f))
            .filter(|f| f != &norm_trigger && !f.is_empty())
            .collect();

        let choreography_json = serde_json::to_string(&normalized_subsequent)?;
        let now = if entry.updated_at.trim().is_empty() {
            crate::graph_backend::helpers::now_ts()
        } else {
            entry.updated_at.clone()
        };

        let inner = self.inner.lock().unwrap();
        inner.sqlite.execute(
            "INSERT INTO procedural_habits (
                id, trigger_file, choreography_json, confidence, occurrences,
                updated_at, tenant_id, workspace_root
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                choreography_json = excluded.choreography_json,
                confidence = MAX(procedural_habits.confidence, excluded.confidence),
                occurrences = procedural_habits.occurrences + 1,
                updated_at = excluded.updated_at",
            params![
                habit_id,
                norm_trigger,
                choreography_json,
                entry.confidence.clamp(0.0, 1.0),
                entry.occurrences.max(1),
                now,
                self.tenant_id,
                inner.workspace_root
            ],
        )?;

        Ok(())
    }

    /// Retrieves all procedural habits for a given trigger file,
    /// ordered by confidence DESC and occurrences DESC.
    pub fn get_procedural_habits_for_file(&self, path: &str) -> Result<Vec<ProceduralHabitEntry>> {
        let norm_path = normalize_rel_path(path);
        let inner = self.inner.lock().unwrap();

        let mut stmt = inner.sqlite.prepare(
            "SELECT id, trigger_file, choreography_json, confidence, occurrences,
                    updated_at, tenant_id, workspace_root
             FROM procedural_habits
             WHERE trigger_file = ?1 AND tenant_id = ?2
             ORDER BY confidence DESC, occurrences DESC",
        )?;

        let rows = stmt.query_map(params![norm_path, self.tenant_id], |row| {
            let ch_json: String = row.get(2)?;
            let subsequent_files: Vec<String> = serde_json::from_str(&ch_json).unwrap_or_default();
            Ok(ProceduralHabitEntry {
                id: row.get(0)?,
                trigger_file: row.get(1)?,
                subsequent_files,
                confidence: row.get(3)?,
                occurrences: row.get(4)?,
                updated_at: row.get(5)?,
                tenant_id: row.get(6)?,
                workspace_root: row.get(7)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Lists active procedural habits across the workspace with a minimum confidence threshold.
    pub fn list_active_habits(&self, min_confidence: f64) -> Result<Vec<ProceduralHabitEntry>> {
        let inner = self.inner.lock().unwrap();

        let mut stmt = inner.sqlite.prepare(
            "SELECT id, trigger_file, choreography_json, confidence, occurrences,
                    updated_at, tenant_id, workspace_root
             FROM procedural_habits
             WHERE confidence >= ?1 AND tenant_id = ?2
             ORDER BY confidence DESC, occurrences DESC",
        )?;

        let rows = stmt.query_map(params![min_confidence, self.tenant_id], |row| {
            let ch_json: String = row.get(2)?;
            let subsequent_files: Vec<String> = serde_json::from_str(&ch_json).unwrap_or_default();
            Ok(ProceduralHabitEntry {
                id: row.get(0)?,
                trigger_file: row.get(1)?,
                subsequent_files,
                confidence: row.get(3)?,
                occurrences: row.get(4)?,
                updated_at: row.get(5)?,
                tenant_id: row.get(6)?,
                workspace_root: row.get(7)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    /// Deletes a procedural habit by ID.
    pub fn delete_procedural_habit(&self, id: &str) -> Result<bool> {
        let inner = self.inner.lock().unwrap();
        let affected = inner.sqlite.execute(
            "DELETE FROM procedural_habits WHERE id = ?1 AND tenant_id = ?2",
            params![id, self.tenant_id],
        )?;
        Ok(affected > 0)
    }
}
