//! Validated server-side `review.inspect` `--fields` section selection (`TASK-92.1`).

/// Which optional sections `review.inspect` should load and include on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewInspectSelection {
    pub body: bool,
    pub rounds: bool,
    pub reviewers: bool,
    pub findings: bool,
}

impl ReviewInspectSelection {
    pub const ALL: Self = Self {
        body: true,
        rounds: true,
        reviewers: true,
        findings: true,
    };

    pub const NONE: Self = Self {
        body: false,
        rounds: false,
        reviewers: false,
        findings: false,
    };

    /// Parses and validates an optional `review.inspect` `fields` allowlist, failing closed on
    /// empty or unknown field paths.
    pub fn try_from_fields(fields: Option<&[String]>) -> Result<Self, String> {
        let Some(fields) = fields else {
            return Ok(Self::ALL);
        };
        if fields.is_empty() {
            return Err("fields list cannot be empty".to_owned());
        }
        let mut sel = Self::NONE;
        for raw in fields {
            sel.try_apply_field(raw.trim())?;
        }
        Ok(sel)
    }

    fn try_apply_field(&mut self, trimmed: &str) -> Result<(), String> {
        if trimmed.is_empty() || trimmed.split('.').any(str::is_empty) {
            return Err(format!("invalid field path `{trimmed}`"));
        }
        let mut parts = trimmed.split('.');
        let root = parts.next().unwrap_or(trimmed);
        let sub = parts.next();
        match root {
            "review" => match sub {
                None => {
                    self.body = true;
                    Ok(())
                }
                Some("body" | "body_markdown") => {
                    self.body = true;
                    Ok(())
                }
                Some(field) if is_review_view_field(field) => Ok(()),
                Some(_) => Err(format!("unknown review field `{trimmed}`")),
            },
            "body" | "body_markdown" => {
                self.body = true;
                Ok(())
            }
            "rounds" => {
                self.rounds = true;
                Ok(())
            }
            "reviewers" => {
                self.reviewers = true;
                Ok(())
            }
            "findings" => {
                self.findings = true;
                Ok(())
            }
            other if is_review_view_field(other) && sub.is_none() => Ok(()),
            _ => Err(format!("unknown review inspect field `{trimmed}`")),
        }
    }
}

fn is_review_view_field(field: &str) -> bool {
    matches!(
        field,
        "id" | "organization_id"
            | "office_code"
            | "number"
            | "key"
            | "project_id"
            | "task_id"
            | "target_kind"
            | "repository_id"
            | "branch"
            | "target_ref"
            | "kind_slug"
            | "category_slug"
            | "title"
            | "body_markdown"
            | "independent"
            | "status"
            | "current_round"
            | "guidance_refs"
            | "criteria"
            | "open_findings"
            | "pin_stale"
            | "closed_reason"
            | "revision"
            | "created_by"
            | "created_at"
            | "updated_at"
    )
}
