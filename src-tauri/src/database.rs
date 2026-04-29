use rusqlite::Connection;

pub(crate) fn init_db(db: &Connection) -> anyhow::Result<()> {
    db.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS repos (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            path TEXT NOT NULL UNIQUE,
            current_branch TEXT,
            default_branch TEXT,
            remote TEXT,
            setup_script TEXT,
            run_script TEXT,
            run_script_mode TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY,
            repo_id TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            path TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'active',
            branch_name TEXT,
            base_branch TEXT,
            checkpoint_id TEXT,
            notes TEXT,
            setup_log_path TEXT,
            run_log_path TEXT,
            archive_commit TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            title TEXT NOT NULL DEFAULT 'Untitled',
            agent_type TEXT NOT NULL DEFAULT 'claude',
            model TEXT,
            permission_mode TEXT NOT NULL DEFAULT 'default',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS session_messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            raw_json TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS terminal_sessions (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            kind TEXT NOT NULL DEFAULT 'command',
            label TEXT NOT NULL DEFAULT '',
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            output TEXT NOT NULL,
            exit_code INTEGER,
            checkpoint_id TEXT,
            started_at INTEGER NOT NULL,
            ended_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pty_terminal_tabs (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            cwd TEXT NOT NULL,
            output TEXT NOT NULL,
            is_running INTEGER NOT NULL DEFAULT 0,
            started_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS diff_comments (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            file_path TEXT NOT NULL,
            line_number INTEGER NOT NULL DEFAULT 0,
            body TEXT NOT NULL,
            is_resolved INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        );
        ",
    )?;
    add_column_if_missing(db, "repos", "current_branch", "TEXT")?;
    add_column_if_missing(db, "repos", "default_branch", "TEXT")?;
    add_column_if_missing(db, "repos", "remote", "TEXT")?;
    add_column_if_missing(db, "repos", "setup_script", "TEXT")?;
    add_column_if_missing(db, "repos", "run_script", "TEXT")?;
    add_column_if_missing(db, "repos", "run_script_mode", "TEXT")?;
    add_column_if_missing(db, "workspaces", "branch_name", "TEXT")?;
    add_column_if_missing(db, "workspaces", "base_branch", "TEXT")?;
    add_column_if_missing(db, "workspaces", "checkpoint_id", "TEXT")?;
    add_column_if_missing(db, "workspaces", "notes", "TEXT")?;
    add_column_if_missing(db, "workspaces", "setup_log_path", "TEXT")?;
    add_column_if_missing(db, "workspaces", "run_log_path", "TEXT")?;
    add_column_if_missing(db, "workspaces", "archive_commit", "TEXT")?;
    add_column_if_missing(
        db,
        "terminal_sessions",
        "kind",
        "TEXT NOT NULL DEFAULT 'command'",
    )?;
    add_column_if_missing(db, "terminal_sessions", "label", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(db, "terminal_sessions", "checkpoint_id", "TEXT")?;
    Ok(())
}

fn add_column_if_missing(
    db: &Connection,
    table: &str,
    column: &str,
    column_type: &str,
) -> anyhow::Result<()> {
    let mut stmt = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }
    db.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}"),
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_db_creates_current_schema() -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;

        for table in [
            "repos",
            "workspaces",
            "sessions",
            "session_messages",
            "settings",
            "terminal_sessions",
            "pty_terminal_tabs",
            "diff_comments",
        ] {
            assert!(table_exists(&db, table)?, "{table} should exist");
        }

        assert_columns(
            &db,
            "repos",
            &[
                "current_branch",
                "default_branch",
                "remote",
                "setup_script",
                "run_script",
                "run_script_mode",
            ],
        )?;
        assert_columns(
            &db,
            "workspaces",
            &[
                "branch_name",
                "base_branch",
                "checkpoint_id",
                "notes",
                "setup_log_path",
                "run_log_path",
                "archive_commit",
            ],
        )?;
        assert_columns(
            &db,
            "terminal_sessions",
            &["kind", "label", "checkpoint_id"],
        )?;
        Ok(())
    }

    #[test]
    fn init_db_migrates_legacy_tables_without_losing_rows() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = Connection::open_in_memory()?;
        db.execute_batch(
            "
            CREATE TABLE repos (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE workspaces (
                id TEXT PRIMARY KEY,
                repo_id TEXT NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE terminal_sessions (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                command TEXT NOT NULL,
                cwd TEXT NOT NULL,
                output TEXT NOT NULL,
                exit_code INTEGER,
                started_at INTEGER NOT NULL,
                ended_at INTEGER NOT NULL
            );
            INSERT INTO repos (id, name, path, created_at, updated_at)
            VALUES ('repo-1', 'repo', '/tmp/repo', 1, 1);
            INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
            VALUES ('workspace-1', 'repo-1', 'workspace', '/tmp/repo', 'active', 1, 1);
            INSERT INTO terminal_sessions (id, workspace_id, command, cwd, output, exit_code, started_at, ended_at)
            VALUES ('terminal-1', 'workspace-1', 'echo ok', '/tmp/repo', 'ok', 0, 1, 1);
            ",
        )?;

        init_db(&db)?;
        init_db(&db)?;

        assert_columns(&db, "repos", &["current_branch", "setup_script"])?;
        assert_columns(&db, "workspaces", &["branch_name", "checkpoint_id"])?;
        assert_columns(
            &db,
            "terminal_sessions",
            &["kind", "label", "checkpoint_id"],
        )?;
        let repo_count: i64 = db.query_row("SELECT COUNT(*) FROM repos", [], |row| row.get(0))?;
        let terminal_kind: String =
            db.query_row("SELECT kind FROM terminal_sessions", [], |row| row.get(0))?;
        let terminal_label: String =
            db.query_row("SELECT label FROM terminal_sessions", [], |row| row.get(0))?;

        assert_eq!(repo_count, 1);
        assert_eq!(terminal_kind, "command");
        assert_eq!(terminal_label, "");
        Ok(())
    }

    fn table_exists(db: &Connection, table: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get(0),
        )?;
        Ok(count == 1)
    }

    fn assert_columns(
        db: &Connection,
        table: &str,
        expected: &[&str],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let columns = columns(db, table)?;
        for column in expected {
            assert!(
                columns.iter().any(|existing| existing == column),
                "{table}.{column} should exist"
            );
        }
        Ok(())
    }

    fn columns(db: &Connection, table: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut stmt = db.prepare(&format!("PRAGMA table_info({table})"))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
        let mut columns = Vec::new();
        for row in rows {
            columns.push(row?);
        }
        Ok(columns)
    }
}
