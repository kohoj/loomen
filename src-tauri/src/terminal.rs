use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

const MAX_SCROLLBACK_BYTES: usize = 200_000;

pub(crate) struct PtySession {
    child: Child,
    master: File,
    output: Arc<Mutex<String>>,
    workspace_id: String,
    cwd: String,
    started_at: i64,
}

impl PtySession {
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(crate) fn start_output_reader(&self) -> Result<(), String> {
        let reader = self
            .master
            .try_clone()
            .map_err(|err| format!("failed to clone PTY master: {err}"))?;
        let output = self.output.clone();
        std::thread::spawn(move || read_pty_output(reader, output));
        Ok(())
    }

    pub(crate) fn write_input(&mut self, input: &str) -> Result<(), String> {
        self.master
            .write_all(input.as_bytes())
            .map_err(|err| err.to_string())?;
        self.master.flush().map_err(|err| err.to_string())
    }

    pub(crate) fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PtyTerminalInfo {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    pub(crate) cwd: String,
    pub(crate) output: String,
    pub(crate) is_running: bool,
    pub(crate) started_at: i64,
}

pub(crate) fn spawn_pty_shell(
    workspace_id: &str,
    cwd: &str,
    started_at: i64,
) -> Result<PtySession, String> {
    let mut master_fd: RawFd = -1;
    let mut slave_fd: RawFd = -1;
    let open_result = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if open_result != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }

    let stdin_fd = unsafe { libc::dup(slave_fd) };
    let stdout_fd = unsafe { libc::dup(slave_fd) };
    let stderr_fd = unsafe { libc::dup(slave_fd) };
    if stdin_fd < 0 || stdout_fd < 0 || stderr_fd < 0 {
        unsafe {
            libc::close(master_fd);
            libc::close(slave_fd);
        }
        return Err(std::io::Error::last_os_error().to_string());
    }

    let mut command = Command::new("/bin/zsh");
    command
        .arg("-il")
        .current_dir(cwd)
        .env("TERM", "xterm-256color")
        .stdin(unsafe { Stdio::from_raw_fd(stdin_fd) })
        .stdout(unsafe { Stdio::from_raw_fd(stdout_fd) })
        .stderr(unsafe { Stdio::from_raw_fd(stderr_fd) });
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::ioctl(0, libc::TIOCSCTTY.into(), 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command
        .spawn()
        .map_err(|err| format!("failed to spawn PTY shell: {err}"))?;
    unsafe {
        libc::close(slave_fd);
    }
    let master = unsafe { File::from_raw_fd(master_fd) };
    Ok(PtySession {
        child,
        master,
        output: Arc::new(Mutex::new(String::new())),
        workspace_id: workspace_id.to_string(),
        cwd: cwd.to_string(),
        started_at,
    })
}

pub(crate) fn terminal_info(id: &str, session: &mut PtySession) -> Result<PtyTerminalInfo, String> {
    let is_running = match session.child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(_) => false,
    };
    let output = session
        .output
        .lock()
        .map_err(|err| err.to_string())?
        .clone();
    Ok(PtyTerminalInfo {
        id: id.to_string(),
        workspace_id: session.workspace_id.clone(),
        cwd: session.cwd.clone(),
        output,
        is_running,
        started_at: session.started_at,
    })
}

pub(crate) fn upsert_pty_terminal_snapshot(
    db: &Connection,
    info: &PtyTerminalInfo,
) -> Result<(), String> {
    db.execute(
        "INSERT INTO pty_terminal_tabs (id, workspace_id, cwd, output, is_running, started_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            workspace_id = excluded.workspace_id,
            cwd = excluded.cwd,
            output = excluded.output,
            is_running = excluded.is_running,
            started_at = excluded.started_at,
            updated_at = excluded.updated_at",
        params![
            info.id,
            info.workspace_id,
            info.cwd,
            info.output,
            if info.is_running { 1 } else { 0 },
            info.started_at,
            now_ms()
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub(crate) fn load_pty_terminal_snapshots(
    db: &Connection,
    workspace_id: &str,
) -> Result<Vec<PtyTerminalInfo>, String> {
    let mut stmt = db
        .prepare(
            "SELECT id, workspace_id, cwd, output, started_at
             FROM pty_terminal_tabs
             WHERE workspace_id = ?1
             ORDER BY started_at ASC",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![workspace_id], |row| {
            Ok(PtyTerminalInfo {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                cwd: row.get(2)?,
                output: row.get(3)?,
                is_running: false,
                started_at: row.get(4)?,
            })
        })
        .map_err(|err| err.to_string())?;
    let mut terminals = Vec::new();
    for row in rows {
        terminals.push(row.map_err(|err| err.to_string())?);
    }
    Ok(terminals)
}

pub(crate) fn load_pty_terminal_snapshot(
    db: &Connection,
    terminal_id: &str,
) -> Result<Option<PtyTerminalInfo>, String> {
    db.query_row(
        "SELECT id, workspace_id, cwd, output, started_at
         FROM pty_terminal_tabs
         WHERE id = ?1",
        params![terminal_id],
        |row| {
            Ok(PtyTerminalInfo {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                cwd: row.get(2)?,
                output: row.get(3)?,
                is_running: false,
                started_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|err| err.to_string())
}

pub(crate) fn delete_pty_terminal_snapshot(
    db: &Connection,
    terminal_id: &str,
) -> Result<(), String> {
    db.execute(
        "DELETE FROM pty_terminal_tabs WHERE id = ?1",
        params![terminal_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn read_pty_output(mut reader: File, output: Arc<Mutex<String>>) {
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buffer[..n]);
                if let Ok(mut out) = output.lock() {
                    out.push_str(&chunk);
                    trim_scrollback(&mut out);
                }
            }
            Err(_) => break,
        }
    }
}

fn trim_scrollback(output: &mut String) {
    let mut keep_from = output.len().saturating_sub(MAX_SCROLLBACK_BYTES);
    if keep_from == 0 {
        return;
    }
    while keep_from < output.len() && !output.is_char_boundary(keep_from) {
        keep_from += 1;
    }
    output.drain(..keep_from);
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn starts_pty_shell_and_captures_scrollback() -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::temp_dir();
        let mut session = spawn_pty_shell("workspace-test", cwd.to_str().unwrap(), now_ms())?;
        session.start_output_reader()?;

        session.write_input("echo PTY_OK\nexit\n")?;
        std::thread::sleep(std::time::Duration::from_millis(700));
        let _ = session.child.wait();
        let info = terminal_info("terminal-test", &mut session)?;
        assert!(
            info.output.contains("PTY_OK"),
            "output was: {}",
            info.output
        );
        Ok(())
    }

    #[test]
    fn pty_terminal_snapshots_rehydrate_scrollback() -> Result<(), Box<dyn std::error::Error>> {
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
        let info = PtyTerminalInfo {
            id: "terminal-1".to_string(),
            workspace_id: "workspace-1".to_string(),
            cwd: "/tmp".to_string(),
            output: "scrollback\nPTY_OK\n".to_string(),
            is_running: true,
            started_at: now,
        };
        upsert_pty_terminal_snapshot(&db, &info)?;

        let snapshots = load_pty_terminal_snapshots(&db, "workspace-1")?;
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, "terminal-1");
        assert_eq!(snapshots[0].is_running, false);
        assert!(snapshots[0].output.contains("PTY_OK"));

        let single = load_pty_terminal_snapshot(&db, "terminal-1")?.expect("snapshot exists");
        assert_eq!(single.cwd, "/tmp");
        delete_pty_terminal_snapshot(&db, "terminal-1")?;
        assert!(load_pty_terminal_snapshot(&db, "terminal-1")?.is_none());
        Ok(())
    }

    #[test]
    fn trim_scrollback_preserves_utf8_boundaries() {
        let mut output = format!("{}DONE", "光".repeat(70_000));
        trim_scrollback(&mut output);
        assert!(output.is_char_boundary(0));
        assert!(output.ends_with("DONE"));
        assert!(output.len() <= MAX_SCROLLBACK_BYTES);
    }
}
