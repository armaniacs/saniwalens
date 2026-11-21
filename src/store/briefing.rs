use super::*;

impl Store {
    /// Bounded `file_edits`: the head of the same time-ordered stream, for
    /// thrash detection over a bounded input.
    pub fn file_edits_briefing(&self, range: TimeRange, limit: usize) -> Result<Vec<FileEdit>> {
        let path = encoding_column("file_edit", EventRole::FilePath);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT s.project, e.session_id, e.{path}, e.started_epoch
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'file_edit' AND e.{path} IS NOT NULL{filter}
             ORDER BY e.started_epoch LIMIT ?"
        );
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(FileEdit {
                    project: row.get(0)?,
                    session_id: row.get(1)?,
                    path: row.get(2)?,
                    epoch: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `work_counts`: the densest `limit` groups in the same order.
    pub fn work_counts_briefing(
        &self,
        kind: &str,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<(String, i64)>> {
        let id = match kind {
            "file_edit" => encoding_column("file_edit", EventRole::FileBasename),
            "bash_cmd" => encoding_column("bash_cmd", EventRole::CommandWord),
            other => anyhow::bail!("no encoding defined for work-event kind {other:?}"),
        };
        let (filter, binds) = range_clause(range, "started_epoch");
        let sql = format!(
            "SELECT {id}, COUNT(*) FROM events WHERE kind = ?1{filter}
             GROUP BY {id} ORDER BY COUNT(*) DESC LIMIT ?"
        );
        let kind_cell = rusqlite::types::Value::Text(kind.to_string());
        let params = range_params(&binds, [kind_cell], limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `error_tool_breakdown`: the densest `limit` rows, same order.
    pub fn error_tool_breakdown_briefing(
        &self,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<ToolBreakdown>> {
        let category = encoding_column("tool_error", EventRole::Category);
        let tool = encoding_column("tool_error", EventRole::OriginatingTool);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT s.project, e.{category}, COALESCE(e.{tool}, 'unknown'), COUNT(*)
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'tool_error'{filter}
             GROUP BY s.project, e.{category}, e.{tool}
             ORDER BY COUNT(*) DESC, s.project, e.{category} LIMIT ?"
        );
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(ToolBreakdown {
                    project: row.get(0)?,
                    category: row.get(1)?,
                    tool: row.get(2)?,
                    count: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `error_counts_by_project`: the densest `limit` cells.
    pub fn error_counts_by_project_briefing(
        &self,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<FrictionCell>> {
        let category = encoding_column("tool_error", EventRole::Category);
        let (filter, binds) = range_clause(range, "e.started_epoch");
        let sql = format!(
            "SELECT s.project, e.{category}, COUNT(*)
             FROM events e JOIN sessions s ON e.session_id = s.id
             WHERE e.kind = 'tool_error'{filter}
             GROUP BY s.project, e.{category}
             ORDER BY COUNT(*) DESC, s.project, e.{category} LIMIT ?"
        );
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(FrictionCell {
                    project: row.get(0)?,
                    category: row.get(1)?,
                    count: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `prompt_behavior_counts`: the most frequent `limit` classes.
    pub fn prompt_behavior_counts_briefing(
        &self,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<(String, i64)>> {
        let behavior = encoding_column("prompt", EventRole::PromptBehavior);
        let (filter, binds) = range_clause(range, "started_epoch");
        let sql = format!(
            "SELECT {behavior}, COUNT(*) FROM events WHERE kind = 'prompt' AND {behavior} IS NOT NULL{filter}
             GROUP BY {behavior} ORDER BY COUNT(*) DESC LIMIT ?"
        );
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `skill_usage`: the most-invoked `limit` skills, same order.
    pub fn skill_usage_briefing(&self, range: TimeRange, limit: usize) -> Result<Vec<SkillUsage>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT surface_id,
                    COUNT(*),
                    SUM(out_tokens),
                    SUM(ctx_growth),
                    SUM(duration_sec)
             FROM events
             WHERE surface_kind = 'skill'{filter}
             GROUP BY surface_id
             ORDER BY COUNT(*) DESC, surface_id LIMIT ?"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(SkillUsage {
                    skill: row.get(0)?,
                    invocations: row.get(1)?,
                    out_tokens: row.get(2)?,
                    ctx_growth: row.get(3)?,
                    duration_sec: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `subagent_by_agent`: the costliest `limit` agent types.
    pub fn subagent_by_agent_briefing(
        &self,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<AgentCost>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT agent, COUNT(*), COALESCE(SUM(out_tokens), 0)
             FROM subagent_runs
             WHERE 1=1{filter}
             GROUP BY agent
             ORDER BY COALESCE(SUM(out_tokens), 0) DESC, COUNT(*) DESC LIMIT ?"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(AgentCost {
                    agent: row.get(0)?,
                    runs: row.get(1)?,
                    out_tokens: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `skill_event_costs`: the first `limit` skill events in
    /// storage order, for time bucketing over a bounded input.
    pub fn skill_event_costs_briefing(
        &self,
        range: TimeRange,
        limit: usize,
    ) -> Result<Vec<EventCost>> {
        let (filter, binds) = range_clause(range, "started_epoch");
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT started_epoch, out_tokens, ctx_growth, duration_sec
             FROM events
             WHERE surface_kind = 'skill'{filter} LIMIT ?"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                Ok(EventCost {
                    started_epoch: row.get(0)?,
                    out_tokens: row.get(1)?,
                    ctx_growth: row.get(2)?,
                    duration_sec: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `effective_catalog`: the first `limit` catalog rows in the
    /// same `(kind, id, scope, project)` order.
    pub fn effective_catalog_briefing(
        &self,
        events_window: TimeRange,
        limit: usize,
    ) -> Result<Vec<CatalogEntry>> {
        let (filter, binds) = range_clause(events_window, "e.started_epoch");
        let params = range_params(&binds, [], limit);
        let mut stmt = self.conn.prepare(&format!(
            "SELECT f.kind, f.id, f.scope, f.project, f.static_tokens,
                    f.startup_tokens, f.load_mode, f.attrs_json, f.description, COUNT(u.event_id)
             FROM surfaces f
             LEFT JOIN (SELECT e.id AS event_id, e.surface_kind, e.surface_id,
                               s.project AS session_project
                        FROM events e JOIN sessions s ON e.session_id = s.id
                        WHERE e.surface_kind IS NOT NULL{filter}) u
               ON u.surface_kind = f.kind AND u.surface_id = f.id
              AND ((f.scope = 'project' AND u.session_project = f.project)
                OR (f.scope = 'global' AND u.session_project NOT IN (
                      SELECT p.project FROM surfaces p
                      WHERE p.kind = f.kind AND p.id = f.id AND p.scope = 'project')))
             GROUP BY f.kind, f.id, f.scope, f.project
             ORDER BY f.kind, f.id, f.scope, f.project LIMIT ?"
        ))?;
        let rows = stmt
            .query_map(params_from_iter(params), |row| {
                let attrs: Option<String> = row.get(7)?;
                Ok(CatalogEntry {
                    kind: row.get(0)?,
                    id: row.get(1)?,
                    scope: row.get(2)?,
                    project: row.get(3)?,
                    static_tokens: row.get(4)?,
                    startup_tokens: row.get(5)?,
                    load_mode: row.get(6)?,
                    uses: row.get(9)?,
                    paths: attrs
                        .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
                        .unwrap_or_default(),
                    description: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Bounded `session_roots`: the first `limit` pairs in root order, with
    /// the slug typed. The untyped [`Store::session_roots`] keeps its tuple
    /// shape for the adapter caller; this twin feeds `FindingsInput` alone.
    pub fn session_roots_briefing(&self, limit: usize) -> Result<Vec<SessionRoot>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT root, project FROM sessions WHERE root <> '' ORDER BY root LIMIT ?",
        )?;
        let rows = stmt
            .query_map([saturating_limit(limit)], |row| {
                Ok(SessionRoot {
                    root: row.get(0)?,
                    project: ProjectSlug(row.get(1)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
