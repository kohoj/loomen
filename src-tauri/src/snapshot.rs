use crate::git::list_git_branches;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Repo {
    id: String,
    name: String,
    path: String,
    current_branch: Option<String>,
    default_branch: Option<String>,
    remote: Option<String>,
    branches: Vec<String>,
    setup_script: Option<String>,
    run_script: Option<String>,
    run_script_mode: Option<String>,
    created_at: i64,
    updated_at: i64,
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Workspace {
    id: String,
    repo_id: String,
    name: String,
    path: String,
    state: String,
    branch_name: Option<String>,
    base_branch: Option<String>,
    checkpoint_id: Option<String>,
    notes: Option<String>,
    setup_log_path: Option<String>,
    run_log_path: Option<String>,
    archive_commit: Option<String>,
    created_at: i64,
    updated_at: i64,
    sessions: Vec<Session>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Session {
    id: String,
    workspace_id: String,
    title: String,
    agent_type: String,
    model: Option<String>,
    permission_mode: String,
    created_at: i64,
    updated_at: i64,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Message {
    id: String,
    session_id: String,
    role: String,
    content: String,
    created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSnapshot {
    db_path: String,
    repos: Vec<Repo>,
}

pub(crate) fn load_snapshot(db: &Connection, db_path: &Path) -> Result<AppSnapshot, String> {
    let mut repos_stmt = db
        .prepare(
            "SELECT id, name, path, current_branch, default_branch, remote, setup_script, run_script, run_script_mode, created_at, updated_at
             FROM repos ORDER BY updated_at DESC",
        )
        .map_err(|err| err.to_string())?;
    let repo_rows = repos_stmt
        .query_map([], |row| {
            Ok(Repo {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
                current_branch: row.get(3)?,
                default_branch: row.get(4)?,
                remote: row.get(5)?,
                branches: Vec::new(),
                setup_script: row.get(6)?,
                run_script: row.get(7)?,
                run_script_mode: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                workspaces: Vec::new(),
            })
        })
        .map_err(|err| err.to_string())?;

    let mut repos = Vec::new();
    for repo in repo_rows {
        let mut repo = repo.map_err(|err| err.to_string())?;
        repo.branches = list_git_branches(
            &repo.path,
            repo.current_branch.as_deref(),
            repo.default_branch.as_deref(),
        );
        repo.workspaces = load_workspaces(db, &repo.id)?;
        repos.push(repo);
    }

    Ok(AppSnapshot {
        db_path: db_path.display().to_string(),
        repos,
    })
}

fn load_workspaces(db: &Connection, repo_id: &str) -> Result<Vec<Workspace>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, repo_id, name, path, state, branch_name, base_branch, checkpoint_id, notes, setup_log_path, run_log_path, archive_commit, created_at, updated_at
             FROM workspaces WHERE repo_id = ?1 ORDER BY updated_at DESC",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![repo_id], |row| {
            Ok(Workspace {
                id: row.get(0)?,
                repo_id: row.get(1)?,
                name: row.get(2)?,
                path: row.get(3)?,
                state: row.get(4)?,
                branch_name: row.get(5)?,
                base_branch: row.get(6)?,
                checkpoint_id: row.get(7)?,
                notes: row.get(8)?,
                setup_log_path: row.get(9)?,
                run_log_path: row.get(10)?,
                archive_commit: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                sessions: Vec::new(),
            })
        })
        .map_err(|err| err.to_string())?;

    let mut workspaces = Vec::new();
    for workspace in rows {
        let mut workspace = workspace.map_err(|err| err.to_string())?;
        workspace.sessions = load_sessions(db, &workspace.id)?;
        workspaces.push(workspace);
    }
    Ok(workspaces)
}

fn load_sessions(db: &Connection, workspace_id: &str) -> Result<Vec<Session>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, workspace_id, title, agent_type, model, permission_mode, created_at, updated_at
             FROM sessions WHERE workspace_id = ?1 ORDER BY updated_at DESC",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![workspace_id], |row| {
            Ok(Session {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                title: row.get(2)?,
                agent_type: row.get(3)?,
                model: row.get(4)?,
                permission_mode: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                messages: Vec::new(),
            })
        })
        .map_err(|err| err.to_string())?;

    let mut sessions = Vec::new();
    for session in rows {
        let mut session = session.map_err(|err| err.to_string())?;
        session.messages = load_messages(db, &session.id)?;
        sessions.push(session);
    }
    Ok(sessions)
}

fn load_messages(db: &Connection, session_id: &str) -> Result<Vec<Message>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, session_id, role, content, created_at
             FROM session_messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?;

    let mut messages = Vec::new();
    for message in rows {
        messages.push(message.map_err(|err| err.to_string())?);
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_db;

    #[test]
    fn snapshot_hydrates_nested_repos_workspaces_sessions_and_messages(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        db.execute(
            "INSERT INTO repos (id, name, path, current_branch, default_branch, remote, created_at, updated_at)
             VALUES ('repo-1', 'repo', '/tmp/missing-loomen-repo', 'feature/ray', 'main', 'origin', 1, 10)",
            [],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, branch_name, base_branch, checkpoint_id, notes, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'Workspace', '/tmp/missing-loomen-workspace', 'active', 'feature/ray', 'main', 'checkpoint-1', 'note', 2, 20)",
            [],
        )?;
        db.execute(
            "INSERT INTO sessions (id, workspace_id, title, agent_type, model, permission_mode, created_at, updated_at)
             VALUES ('session-1', 'workspace-1', 'Session', 'codex', 'gpt-5-codex', 'default', 3, 30)",
            [],
        )?;
        db.execute(
            "INSERT INTO session_messages (id, session_id, role, content, created_at)
             VALUES ('message-2', 'session-1', 'assistant', 'second', 5)",
            [],
        )?;
        db.execute(
            "INSERT INTO session_messages (id, session_id, role, content, created_at)
             VALUES ('message-1', 'session-1', 'user', 'first', 4)",
            [],
        )?;

        let snapshot = load_snapshot(&db, Path::new("/tmp/loomen.db"))?;

        assert_eq!(snapshot.db_path, "/tmp/loomen.db");
        assert_eq!(snapshot.repos.len(), 1);
        assert_eq!(snapshot.repos[0].name, "repo");
        assert_eq!(
            snapshot.repos[0].branches[..3],
            ["feature/ray", "main", "HEAD"]
        );
        assert_eq!(snapshot.repos[0].workspaces.len(), 1);
        assert_eq!(snapshot.repos[0].workspaces[0].name, "Workspace");
        assert_eq!(snapshot.repos[0].workspaces[0].sessions.len(), 1);
        assert_eq!(snapshot.repos[0].workspaces[0].sessions[0].title, "Session");
        assert_eq!(
            snapshot.repos[0].workspaces[0].sessions[0].messages[0].content,
            "first"
        );
        assert_eq!(
            snapshot.repos[0].workspaces[0].sessions[0].messages[1].content,
            "second"
        );
        Ok(())
    }

    #[test]
    fn snapshot_orders_recent_repos_workspaces_and_sessions_first(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        for (id, updated_at) in [("repo-old", 1), ("repo-new", 2)] {
            db.execute(
                "INSERT INTO repos (id, name, path, created_at, updated_at)
                 VALUES (?1, ?1, ?1, 1, ?2)",
                params![id, updated_at],
            )?;
        }
        for (id, updated_at) in [("workspace-old", 10), ("workspace-new", 20)] {
            db.execute(
                "INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
                 VALUES (?1, 'repo-new', ?1, ?1, 'active', 1, ?2)",
                params![id, updated_at],
            )?;
        }
        for (id, workspace_id, updated_at) in [
            ("session-old", "workspace-new", 100),
            ("session-new", "workspace-new", 200),
        ] {
            db.execute(
                "INSERT INTO sessions (id, workspace_id, title, agent_type, permission_mode, created_at, updated_at)
                 VALUES (?1, ?2, ?1, 'claude', 'default', 1, ?3)",
                params![id, workspace_id, updated_at],
            )?;
        }

        let snapshot = load_snapshot(&db, Path::new("/tmp/loomen.db"))?;

        assert_eq!(snapshot.repos[0].id, "repo-new");
        assert_eq!(snapshot.repos[0].workspaces[0].id, "workspace-new");
        assert_eq!(
            snapshot.repos[0].workspaces[0].sessions[0].id,
            "session-new"
        );
        Ok(())
    }
}
