use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiffOutput {
    pub(crate) workspace_id: String,
    pub(crate) checkpoint_id: Option<String>,
    pub(crate) diff: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiffFile {
    pub(crate) path: String,
    pub(crate) status: String,
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
    pub(crate) patch: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiffComment {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    pub(crate) file_path: String,
    pub(crate) line_number: i64,
    pub(crate) body: String,
    pub(crate) is_resolved: bool,
    pub(crate) created_at: i64,
}

pub(crate) fn parse_diff_files(raw: &str) -> Vec<DiffFile> {
    let mut files = Vec::new();
    let mut current_path = String::new();
    let mut current_patch = Vec::new();
    let mut additions = 0usize;
    let mut deletions = 0usize;

    let flush = |files: &mut Vec<DiffFile>,
                 current_path: &mut String,
                 current_patch: &mut Vec<String>,
                 additions: &mut usize,
                 deletions: &mut usize| {
        if current_path.is_empty() {
            return;
        }
        let status = if *additions > 0 && *deletions > 0 {
            "modified"
        } else if *additions > 0 {
            "added"
        } else if *deletions > 0 {
            "deleted"
        } else {
            "changed"
        };
        files.push(DiffFile {
            path: current_path.clone(),
            status: status.to_string(),
            additions: *additions,
            deletions: *deletions,
            patch: current_patch.join("\n"),
        });
        current_path.clear();
        current_patch.clear();
        *additions = 0;
        *deletions = 0;
    };

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(
                &mut files,
                &mut current_path,
                &mut current_patch,
                &mut additions,
                &mut deletions,
            );
            let path = rest.split(" b/").nth(1).unwrap_or(rest).trim().to_string();
            current_path = path;
        }
        if line.starts_with('+') && !line.starts_with("+++") {
            additions += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            deletions += 1;
        }
        if !current_path.is_empty() {
            current_patch.push(line.to_string());
        }
    }
    flush(
        &mut files,
        &mut current_path,
        &mut current_patch,
        &mut additions,
        &mut deletions,
    );
    files
}

pub(crate) fn add_diff_comment_for_db(
    db: &Connection,
    workspace_id: &str,
    file_path: &str,
    line_number: i64,
    body: &str,
) -> Result<Vec<DiffComment>, String> {
    insert_diff_comment(db, workspace_id, file_path, line_number, body)?;
    list_diff_comments_for_db(db, workspace_id)
}

pub(crate) fn add_diff_comment_from_params(
    db: &Connection,
    workspace_id: &str,
    params: &Value,
) -> Result<Vec<DiffComment>, String> {
    let file_path = params
        .get("filePath")
        .or_else(|| params.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let line_number = params
        .get("lineNumber")
        .or_else(|| params.get("line_number"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let body = params
        .get("body")
        .or_else(|| params.get("comment"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if body.is_empty() {
        return Err("diffComment body is empty".to_string());
    }
    insert_diff_comment(db, workspace_id, file_path, line_number, &body)?;
    list_diff_comments_for_db(db, workspace_id)
}

pub(crate) fn resolve_diff_comment_for_db(
    db: &Connection,
    comment_id: &str,
    workspace_id: &str,
) -> Result<Vec<DiffComment>, String> {
    db.execute(
        "UPDATE diff_comments SET is_resolved = 1 WHERE id = ?1 AND workspace_id = ?2",
        params![comment_id, workspace_id],
    )
    .map_err(|err| err.to_string())?;
    list_diff_comments_for_db(db, workspace_id)
}

pub(crate) fn list_diff_comments_for_db(
    db: &Connection,
    workspace_id: &str,
) -> Result<Vec<DiffComment>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, workspace_id, file_path, line_number, body, is_resolved, created_at
             FROM diff_comments WHERE workspace_id = ?1 ORDER BY created_at DESC",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![workspace_id], |row| {
            Ok(DiffComment {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                file_path: row.get(2)?,
                line_number: row.get(3)?,
                body: row.get(4)?,
                is_resolved: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
            })
        })
        .map_err(|err| err.to_string())?;
    let mut comments = Vec::new();
    for row in rows {
        comments.push(row.map_err(|err| err.to_string())?);
    }
    Ok(comments)
}

fn insert_diff_comment(
    db: &Connection,
    workspace_id: &str,
    file_path: &str,
    line_number: i64,
    body: &str,
) -> Result<(), String> {
    db.execute(
        "INSERT INTO diff_comments (id, workspace_id, file_path, line_number, body, is_resolved, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
        params![
            Uuid::new_v4().to_string(),
            workspace_id,
            file_path,
            line_number,
            body,
            now_ms()
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn parses_structured_diff_files() {
        let raw = r#"diff --git a/README.md b/README.md
index ce01362..cc628cc 100644
--- a/README.md
+++ b/README.md
@@ -1 +1,2 @@
-hello
+hello
+world
"#;
        let files = parse_diff_files(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "README.md");
        assert_eq!(files[0].additions, 2);
        assert_eq!(files[0].deletions, 1);
        assert!(files[0].patch.contains("@@ -1 +1,2 @@"));
    }

    #[test]
    fn parses_multiple_diff_files_with_statuses() {
        let raw = r#"diff --git a/new.md b/new.md
new file mode 100644
--- /dev/null
+++ b/new.md
@@ -0,0 +1 @@
+new
diff --git a/old.md b/old.md
deleted file mode 100644
--- a/old.md
+++ /dev/null
@@ -1 +0,0 @@
-old
"#;
        let files = parse_diff_files(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "new.md");
        assert_eq!(files[0].status, "added");
        assert_eq!(files[1].path, "old.md");
        assert_eq!(files[1].status, "deleted");
    }

    #[test]
    fn diff_comments_preserve_command_body_and_resolve() -> Result<(), Box<dyn std::error::Error>> {
        let db = review_test_db()?;
        let comments =
            add_diff_comment_for_db(&db, "workspace-1", "README.md", 7, "  keep spacing  ")?;
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].body, "  keep spacing  ");
        assert!(!comments[0].is_resolved);

        let resolved = resolve_diff_comment_for_db(&db, &comments[0].id, "workspace-1")?;
        assert!(resolved[0].is_resolved);
        Ok(())
    }

    #[test]
    fn reverse_diff_comment_params_trim_and_validate_body() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = review_test_db()?;
        let comments = add_diff_comment_from_params(
            &db,
            "workspace-1",
            &serde_json::json!({
                "filePath": "src/main.rs",
                "lineNumber": 42,
                "body": "  tighten this  "
            }),
        )?;
        assert_eq!(comments[0].file_path, "src/main.rs");
        assert_eq!(comments[0].line_number, 42);
        assert_eq!(comments[0].body, "tighten this");

        let err =
            add_diff_comment_from_params(&db, "workspace-1", &serde_json::json!({ "body": "   " }))
                .expect_err("blank reverse RPC body should be rejected");
        assert_eq!(err, "diffComment body is empty");
        Ok(())
    }

    fn review_test_db() -> Result<Connection, Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        crate::init_db(&db)?;
        let now = now_ms();
        db.execute(
            "INSERT INTO repos (id, name, path, setup_script, run_script, run_script_mode, created_at, updated_at)
             VALUES ('repo-1', 'repo', '/tmp', '', '', 'concurrent', ?1, ?1)",
            params![now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'workspace', '/tmp', 'active', ?1, ?1)",
            params![now],
        )?;
        Ok(db)
    }
}
