use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

mod database;
pub(crate) use database::init_db;
mod git;
use git::{
    branch_exists_for_worktree, checkpoint_diff, create_git_worktree, detect_default_branch,
    git_output, resolve_git_root, save_checkpoint,
};
mod github;
use github::{
    create_pull_request_for_cwd, get_pull_request_info_for_cwd, rerun_failed_checks_for_branch,
    update_pull_request_for_cwd, PullRequestInfo,
};
mod pulse;
use pulse::{
    command_label, list_pulse_evidence_for_db, named_pulses_for_path, run_shell_command,
    store_terminal_run, NamedPulse, TerminalRun,
};
mod review;
use review::{
    add_diff_comment_for_db, add_diff_comment_from_params, list_diff_comments_for_db,
    parse_diff_files, resolve_diff_comment_for_db, DiffComment, DiffFile, DiffOutput,
};
mod settings;
use settings::{
    default_model_for_agent, default_permission_mode, load_settings, save_settings, AppSettings,
};
mod snapshot;
use snapshot::{load_snapshot, AppSnapshot};
mod sidecar;
use sidecar::{
    cancel_query as cancel_sidecar_query, send_query_to_sidecar as send_query_to_sidecar_socket,
    sidecar_health, sidecar_rpc as sidecar_socket_rpc, sidecar_socket_path, SidecarProcess,
    SidecarQuery, SidecarRuntimeSettings,
};
mod terminal;
use terminal::{
    delete_pty_terminal_snapshot, load_pty_terminal_snapshot, load_pty_terminal_snapshots,
    spawn_pty_shell, terminal_info, upsert_pty_terminal_snapshot, PtySession, PtyTerminalInfo,
};

struct AppState {
    db: Mutex<Connection>,
    db_path: PathBuf,
    rebuild_root: PathBuf,
    sidecar: Mutex<Option<SidecarProcess>>,
    ptys: Mutex<HashMap<String, PtySession>>,
    spotlighters: Mutex<HashMap<String, SpotlighterProcess>>,
    approvals: Mutex<HashMap<String, mpsc::Sender<ToolApprovalDecision>>>,
}

struct SpotlighterProcess {
    child: Child,
    workspace_id: String,
    workspace_path: String,
    root_path: String,
    started_at: i64,
}

impl Drop for SpotlighterProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WeavePreview {
    repo_id: String,
    repo_path: String,
    repo_name: String,
    workspace_name: String,
    branch_leaf: String,
    branch_name: String,
    base_branch: String,
    worktree_path: String,
    checkpoint_id: String,
    path_exists: bool,
    path_is_empty: bool,
    can_create: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkspacePlan {
    workspace_name: String,
    branch_leaf: String,
    branch_name: String,
    base_branch: String,
    worktree_path: String,
    checkpoint_id: String,
    path_exists: bool,
    path_is_empty: bool,
    can_create: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchHealth {
    status: String,
    generated_at: i64,
    db_path: String,
    rebuild_root: String,
    checks: Vec<LaunchHealthCheck>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchHealthCheck {
    id: String,
    label: String,
    status: String,
    detail: String,
    path: Option<String>,
    version: Option<String>,
    required: bool,
    remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchExecutable {
    path: Option<String>,
    source: String,
    problem: Option<String>,
    checked_candidates: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeverCleanupPreview {
    workspace_id: String,
    workspace_name: String,
    state: String,
    branch_name: Option<String>,
    branch_exists: Option<bool>,
    worktree_path: String,
    worktree_exists: bool,
    setup_log_path: Option<String>,
    run_log_path: Option<String>,
    has_setup_log: bool,
    has_run_log: bool,
    archive_commit: Option<String>,
    session_count: i64,
    terminal_run_count: i64,
    terminal_tab_count: i64,
    diff_comment_count: i64,
    database_record_count: i64,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    path: String,
    name: String,
    kind: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilePreview {
    workspace_id: String,
    path: String,
    content: String,
    is_binary: bool,
    truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceSearchMatch {
    path: String,
    line: usize,
    column: usize,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SpotlighterInfo {
    workspace_id: String,
    workspace_path: String,
    root_path: String,
    is_running: bool,
    started_at: i64,
}

struct QueryContext {
    agent_type: String,
    cwd: String,
    model: Option<String>,
    permission_mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueryEvent {
    session_id: String,
    text: Option<String>,
    error: Option<String>,
    done: bool,
}

struct ToolApprovalDecision {
    approved: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolApprovalRequest {
    approval_id: String,
    session_id: String,
    tool_name: String,
    input: Value,
    permission_mode: String,
    requested_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QuerySessionEvent {
    session_id: String,
    event: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContextUsage {
    session_id: String,
    used_tokens: i64,
    max_tokens: i64,
    percent: f64,
}

#[tauri::command]
fn get_state(state: State<AppState>) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    load_settings(&db).map_err(|err| err.to_string())
}

#[tauri::command]
fn update_settings(settings: AppSettings, state: State<AppState>) -> Result<AppSettings, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    save_settings(&db, &settings).map_err(|err| err.to_string())?;
    load_settings(&db).map_err(|err| err.to_string())
}

#[tauri::command]
fn open_workspace_in_finder(workspace_id: String, state: State<AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let path = workspace_path(&db, &workspace_id)?;
    open_path_in_finder(&path)
}

#[tauri::command]
fn open_repo_in_finder(repo_id: String, state: State<AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let path = db
        .query_row(
            "SELECT path FROM repos WHERE id = ?1",
            params![repo_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "repo not found".to_string())?;
    open_path_in_finder(&path)
}

#[tauri::command]
fn add_repo(path: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let clean_path = resolve_git_root(path.trim())?;
    let current_branch = git_output(&clean_path, &["branch", "--show-current"]).ok();
    let default_branch = detect_default_branch(&clean_path);
    let remote = git_output(&clean_path, &["config", "--get", "remote.origin.url"]).ok();
    let now = now_ms();
    let repo_id = db
        .query_row(
            "SELECT id FROM repos WHERE path = ?1",
            params![clean_path],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| {
            let id = Uuid::new_v4().to_string();
            let name = path_name(&clean_path);
            db.execute(
                "INSERT INTO repos (id, name, path, current_branch, default_branch, remote, setup_script, run_script, run_script_mode, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', '', 'concurrent', ?7, ?8)",
                params![id, name, clean_path, current_branch, default_branch, remote, now, now],
            )
            .expect("insert repo");
            id
        });

    db.execute(
        "UPDATE repos SET current_branch = ?2, default_branch = ?3, remote = ?4, updated_at = ?5 WHERE id = ?1",
        params![repo_id, current_branch, default_branch, remote, now],
    )
    .map_err(|err| err.to_string())?;
    ensure_workspace(
        &db,
        &repo_id,
        "main",
        &clean_path,
        "active",
        current_branch.as_deref(),
        current_branch.as_deref().or(default_branch.as_deref()),
        None,
        now,
    )?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn preview_workspace(
    repo_id: String,
    name: String,
    path: String,
    base_branch: Option<String>,
    state: State<AppState>,
) -> Result<WeavePreview, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let settings = load_settings(&db).map_err(|err| err.to_string())?;
    let (repo_path, repo_name, current_branch, default_branch) =
        repo_workspace_context(&db, &repo_id)?;
    let plan = build_workspace_plan(
        &repo_path,
        &repo_name,
        current_branch.as_deref(),
        default_branch.as_deref(),
        &settings,
        &name,
        &path,
        base_branch.as_deref(),
        None,
    );
    Ok(WeavePreview {
        repo_id,
        repo_path,
        repo_name,
        workspace_name: plan.workspace_name,
        branch_leaf: plan.branch_leaf,
        branch_name: plan.branch_name,
        base_branch: plan.base_branch,
        worktree_path: plan.worktree_path,
        checkpoint_id: plan.checkpoint_id,
        path_exists: plan.path_exists,
        path_is_empty: plan.path_is_empty,
        can_create: plan.can_create,
        warnings: plan.warnings,
    })
}

#[tauri::command]
fn create_workspace(
    repo_id: String,
    name: String,
    path: String,
    base_branch: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let settings = load_settings(&db).map_err(|err| err.to_string())?;
    let (repo_path, repo_name, current_branch, default_branch) =
        repo_workspace_context(&db, &repo_id)?;
    let plan = build_workspace_plan(
        &repo_path,
        &repo_name,
        current_branch.as_deref(),
        default_branch.as_deref(),
        &settings,
        &name,
        &path,
        base_branch.as_deref(),
        None,
    );
    if !plan.can_create {
        return Err(plan.warnings.join("; "));
    }

    create_git_worktree(
        &repo_path,
        &plan.worktree_path,
        &plan.branch_name,
        &plan.base_branch,
    )?;
    let checkpoint_id = save_checkpoint(&plan.worktree_path, &plan.checkpoint_id).ok();
    ensure_workspace(
        &db,
        &repo_id,
        &plan.workspace_name,
        &plan.worktree_path,
        "active",
        Some(&plan.branch_name),
        Some(&plan.base_branch),
        checkpoint_id.as_deref(),
        now_ms(),
    )?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn create_session(
    workspace_id: String,
    agent_type: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let settings = load_settings(&db).map_err(|err| err.to_string())?;
    let now = now_ms();
    let id = Uuid::new_v4().to_string();
    let normalized_agent = if agent_type == "codex" {
        "codex"
    } else {
        "claude"
    };
    let model = default_model_for_agent(&settings, normalized_agent);
    let permission_mode = default_permission_mode(&settings);
    db.execute(
        "INSERT INTO sessions (id, workspace_id, title, agent_type, model, permission_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            workspace_id,
            "Untitled",
            normalized_agent,
            model,
            permission_mode,
            now,
            now
        ],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn close_session(session_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    db.execute(
        "DELETE FROM session_messages WHERE session_id = ?1",
        params![session_id],
    )
    .map_err(|err| err.to_string())?;
    db.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
        .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn send_query(
    session_id: String,
    prompt: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let prompt = prompt.trim().to_string();
    let now = now_ms();
    let context = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        insert_message(&db, &session_id, "user", &prompt, now)?;
        load_query_context(&db, &session_id)?
    };

    let sidecar_message = send_query_to_sidecar(
        &state,
        &session_id,
        &prompt,
        &context.agent_type,
        &context.cwd,
        context.model.as_deref(),
        &context.permission_mode,
    )
    .unwrap_or_else(|err| {
        format!(
            "[sidecar unavailable]\n\n{}\n\nThe user message was still persisted locally.",
            err
        )
    });
    let db = state.db.lock().map_err(|err| err.to_string())?;
    insert_message(&db, &session_id, "assistant", &sidecar_message, now_ms())?;
    db.execute(
        "UPDATE sessions SET title = CASE WHEN title = 'Untitled' THEN ?2 ELSE title END, updated_at = ?3 WHERE id = ?1",
        params![session_id, title_from_prompt(&prompt), now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn start_query(
    session_id: String,
    prompt: String,
    app: AppHandle,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt is empty".to_string());
    }
    let context = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        insert_message(&db, &session_id, "user", &prompt, now_ms())?;
        db.execute(
            "UPDATE sessions SET title = CASE WHEN title = 'Untitled' THEN ?2 ELSE title END, updated_at = ?3 WHERE id = ?1",
            params![session_id, title_from_prompt(&prompt), now_ms()],
        )
        .map_err(|err| err.to_string())?;
        let context = load_query_context(&db, &session_id)?;
        let snapshot = load_snapshot(&db, &state.db_path)?;
        (context, snapshot)
    };

    let QueryContext {
        agent_type,
        cwd,
        model,
        permission_mode,
    } = context.0;
    let snapshot = context.1;
    let app_handle = app.clone();
    let worker_session_id = session_id.clone();
    std::thread::spawn(move || {
        let managed = app_handle.state::<AppState>();
        let _ = app_handle.emit(
            "loomen-query-started",
            QueryEvent {
                session_id: worker_session_id.clone(),
                text: None,
                error: None,
                done: false,
            },
        );
        let approval_app_handle = app_handle.clone();
        let message_app_handle = app_handle.clone();
        let event_app_handle = app_handle.clone();
        let event_session_id = worker_session_id.clone();
        let result = send_query_to_sidecar_streaming(
            &managed,
            Some(&approval_app_handle),
            &worker_session_id,
            &prompt,
            &agent_type,
            &cwd,
            model.as_deref(),
            &permission_mode,
            |text| {
                let _ = message_app_handle.emit(
                    "loomen-query-message",
                    QueryEvent {
                        session_id: worker_session_id.clone(),
                        text: Some(text),
                        error: None,
                        done: false,
                    },
                );
            },
            |event| {
                let _ = event_app_handle.emit(
                    "loomen-query-session-event",
                    QuerySessionEvent {
                        session_id: event_session_id.clone(),
                        event,
                    },
                );
            },
        );
        let (final_text, error) = match result {
            Ok(text) => (text, None),
            Err(err) => (
                format!(
                    "[sidecar unavailable]\n\n{}\n\nThe user message was still persisted locally.",
                    err
                ),
                Some(err),
            ),
        };
        if let Ok(db) = managed.db.lock() {
            let _ = insert_message(&db, &worker_session_id, "assistant", &final_text, now_ms());
            let _ = db.execute(
                "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
                params![&worker_session_id, now_ms()],
            );
        }
        let _ = app_handle.emit(
            "loomen-query-finished",
            QueryEvent {
                session_id: worker_session_id,
                text: Some(final_text),
                error,
                done: true,
            },
        );
    });

    Ok(snapshot)
}

#[tauri::command]
fn update_session_settings(
    session_id: String,
    model: String,
    permission_mode: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let normalized_permission = match permission_mode.as_str() {
        "acceptEdits" | "auto" | "bypassPermissions" | "default" | "dontAsk" | "plan" => {
            permission_mode
        }
        _ => "default".to_string(),
    };
    db.execute(
        "UPDATE sessions SET model = ?2, permission_mode = ?3, updated_at = ?4 WHERE id = ?1",
        params![session_id, model.trim(), normalized_permission, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn get_context_usage(session_id: String, state: State<AppState>) -> Result<ContextUsage, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    context_usage_for_db(&db, &session_id)
}

#[tauri::command]
fn resolve_tool_approval(
    approval_id: String,
    approved: bool,
    state: State<AppState>,
) -> Result<(), String> {
    let sender = state
        .approvals
        .lock()
        .map_err(|err| err.to_string())?
        .remove(&approval_id)
        .ok_or_else(|| "approval request not found".to_string())?;
    sender
        .send(ToolApprovalDecision { approved })
        .map_err(|err| err.to_string())
}

#[tauri::command]
fn get_db_path(state: State<AppState>) -> String {
    state.db_path.display().to_string()
}

#[tauri::command]
fn get_launch_health(state: State<AppState>) -> Result<LaunchHealth, String> {
    let (settings, database_ok) = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        let database_ok = db
            .query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
            .is_ok();
        let settings = load_settings(&db).map_err(|err| err.to_string())?;
        (settings, database_ok)
    };

    let mut checks = Vec::new();
    checks.push(path_health_check(
        "database",
        "SQLite database",
        &state.db_path,
        true,
        database_ok,
        "Local state is readable.",
        "Loomen opened the database path but could not read from it.",
        "Check file permissions or move the database out of a restricted directory.",
    ));
    checks.push(path_health_check(
        "rebuildRoot",
        "Rebuild root",
        &state.rebuild_root,
        true,
        state.rebuild_root.exists(),
        "Application source root is visible.",
        "The rebuild root path is missing.",
        "Run Loomen from a complete checkout.",
    ));
    checks.push(command_health_check(
        "git",
        "Git",
        "git",
        &["--version"],
        true,
        "Install Git and keep it on PATH.",
    ));
    checks.push(command_health_check(
        "bun",
        "Bun",
        "bun",
        &["--version"],
        true,
        "Install Bun and keep it on PATH so the sidecar can start.",
    ));
    checks.push(command_health_check(
        "gh",
        "GitHub CLI",
        "gh",
        &["--version"],
        false,
        "Install gh and run gh auth login to enable PR and checks features.",
    ));
    checks.push(agent_health_check(
        "claude",
        "Claude Code",
        "claude",
        &settings.claude_executable_path,
        "LOOMEN_CLAUDE_BIN",
        "Set Claude Code executable path in Advanced settings or LOOMEN_CLAUDE_BIN.",
    ));
    checks.push(agent_health_check(
        "codex",
        "Codex",
        "codex",
        &settings.codex_executable_path,
        "LOOMEN_CODEX_BIN",
        "Set Codex executable path in Advanced settings or LOOMEN_CODEX_BIN.",
    ));
    {
        let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
        checks.push(sidecar_launch_health(&mut sidecar));
    }

    Ok(LaunchHealth {
        status: launch_health_status(&checks),
        generated_at: now_ms(),
        db_path: state.db_path.display().to_string(),
        rebuild_root: state.rebuild_root.display().to_string(),
        checks,
    })
}

#[tauri::command]
fn sidecar_status(state: State<AppState>) -> Result<String, String> {
    let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
    Ok(sidecar_socket_path(&state.rebuild_root, &mut sidecar)?
        .display()
        .to_string())
}

#[tauri::command]
fn workspace_init(workspace_id: String, state: State<AppState>) -> Result<Value, String> {
    let (cwd, settings) = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        (
            workspace_path(&db, &workspace_id)?,
            load_settings(&db).map_err(|err| err.to_string())?,
        )
    };
    sidecar_rpc(
        &state,
        "workspaceInit",
        serde_json::json!({
            "id": workspace_id,
            "options": {
                "cwd": cwd,
                "claudeExecutablePath": settings.claude_executable_path,
                "codexExecutablePath": settings.codex_executable_path
            }
        }),
        Duration::from_secs(10),
    )
}

#[tauri::command]
fn claude_auth_status(state: State<AppState>) -> Result<Value, String> {
    let settings = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        load_settings(&db).map_err(|err| err.to_string())?
    };
    sidecar_rpc(
        &state,
        "claudeAuth",
        serde_json::json!({
            "id": "auth",
            "options": {
                "claudeExecutablePath": settings.claude_executable_path
            }
        }),
        Duration::from_secs(12),
    )
}

#[tauri::command]
fn cancel_query(session_id: String, state: State<AppState>) -> Result<(), String> {
    let socket_path = {
        let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
        sidecar_socket_path(&state.rebuild_root, &mut sidecar)?
    };
    cancel_sidecar_query(&socket_path, &session_id)
}

#[tauri::command]
fn update_workspace_notes(
    workspace_id: String,
    notes: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    db.execute(
        "UPDATE workspaces SET notes = ?2, updated_at = ?3 WHERE id = ?1",
        params![workspace_id, notes, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn update_repo_scripts(
    repo_id: String,
    setup_script: String,
    run_script: String,
    run_script_mode: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    db.execute(
        "UPDATE repos SET setup_script = ?2, run_script = ?3, run_script_mode = ?4, updated_at = ?5 WHERE id = ?1",
        params![repo_id, setup_script, run_script, run_script_mode, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn run_workspace_setup(
    workspace_id: String,
    state: State<AppState>,
) -> Result<TerminalRun, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (cwd, script) = workspace_script(&db, &workspace_id, "setup_script")?;
    if script.trim().is_empty() {
        return Err("setup script is empty".to_string());
    }
    db.execute(
        "UPDATE workspaces SET state = 'setting-up', updated_at = ?2 WHERE id = ?1",
        params![workspace_id, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    let checkpoint_id = workspace_checkpoint_id(&db, &workspace_id)?;
    let run = run_shell_command(
        &workspace_id,
        &cwd,
        &script,
        "setup",
        "Setup script",
        checkpoint_id.as_deref(),
    )?;
    let log_path = write_lifecycle_log(&workspace_id, "setup", &run.output)?;
    let next_state = setup_state_for_exit(run.exit_code);
    db.execute(
        "UPDATE workspaces SET state = ?2, setup_log_path = ?3, updated_at = ?4 WHERE id = ?1",
        params![workspace_id, next_state, log_path, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    store_terminal_run(&db, &run)?;
    Ok(run)
}

#[tauri::command]
fn run_workspace_run_script(
    workspace_id: String,
    state: State<AppState>,
) -> Result<TerminalRun, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (cwd, script) = workspace_script(&db, &workspace_id, "run_script")?;
    if script.trim().is_empty() {
        return Err("run script is empty".to_string());
    }
    let checkpoint_id = workspace_checkpoint_id(&db, &workspace_id)?;
    let run = run_shell_command(
        &workspace_id,
        &cwd,
        &script,
        "run",
        "Run script",
        checkpoint_id.as_deref(),
    )?;
    let log_path = write_lifecycle_log(&workspace_id, "run", &run.output)?;
    db.execute(
        "UPDATE workspaces SET run_log_path = ?2, updated_at = ?3 WHERE id = ?1",
        params![workspace_id, log_path, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    store_terminal_run(&db, &run)?;
    Ok(run)
}

#[tauri::command]
fn archive_workspace(workspace_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let path = workspace_path(&db, &workspace_id)?;
    let archive_commit = git_output(&path, &["rev-parse", "HEAD"]).ok();
    db.execute(
        "UPDATE workspaces SET state = 'archived', archive_commit = ?2, updated_at = ?3 WHERE id = ?1",
        params![workspace_id, archive_commit, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn restore_workspace(workspace_id: String, state: State<AppState>) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    db.execute(
        "UPDATE workspaces SET state = 'active', updated_at = ?2 WHERE id = ?1",
        params![workspace_id, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn preview_sever_cleanup(
    workspace_id: String,
    state: State<AppState>,
) -> Result<SeverCleanupPreview, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    sever_cleanup_preview_for_db(&db, &workspace_id)
}

#[tauri::command]
fn save_workspace_checkpoint(
    workspace_id: String,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let workspace_path = workspace_path(&db, &workspace_id)?;
    let checkpoint_id = format!("workspace-{}-{}", workspace_id, now_ms());
    let saved_id = save_checkpoint(&workspace_path, &checkpoint_id)?;
    db.execute(
        "UPDATE workspaces SET checkpoint_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![workspace_id, saved_id, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    load_snapshot(&db, &state.db_path)
}

#[tauri::command]
fn get_workspace_diff(workspace_id: String, state: State<AppState>) -> Result<DiffOutput, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (workspace_path, checkpoint_id, base_branch) = db
        .query_row(
            "SELECT path, checkpoint_id, base_branch FROM workspaces WHERE id = ?1",
            params![workspace_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    let diff = match checkpoint_id.as_deref() {
        Some(id) if !id.is_empty() => checkpoint_diff(&workspace_path, id, "--stat")
            .or_else(|_| checkpoint_diff(&workspace_path, id, "current")),
        _ => git_output(
            &workspace_path,
            &["diff", "--stat", base_branch.as_deref().unwrap_or("HEAD")],
        ),
    }
    .unwrap_or_else(|err| format!("diff unavailable: {err}"));

    Ok(DiffOutput {
        workspace_id,
        checkpoint_id,
        diff: if diff.trim().is_empty() {
            "No diff against the current checkpoint/base.".to_string()
        } else {
            diff
        },
    })
}

#[tauri::command]
fn run_terminal_command(
    workspace_id: String,
    command: String,
    state: State<AppState>,
) -> Result<TerminalRun, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    let checkpoint_id = workspace_checkpoint_id(&db, &workspace_id)?;
    let label = command_label(&command);
    let run = run_shell_command(
        &workspace_id,
        &cwd,
        &command,
        "command",
        &label,
        checkpoint_id.as_deref(),
    )?;
    store_terminal_run(&db, &run)?;
    Ok(run)
}

#[tauri::command]
fn list_pulse_evidence(
    workspace_id: String,
    limit: i64,
    state: State<AppState>,
) -> Result<Vec<TerminalRun>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    list_pulse_evidence_for_db(&db, &workspace_id, limit)
}

#[tauri::command]
fn list_named_pulses(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<NamedPulse>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    named_pulses_for_path(&cwd)
}

#[tauri::command]
fn run_named_pulse(
    workspace_id: String,
    pulse_id: String,
    state: State<AppState>,
) -> Result<TerminalRun, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    let checkpoint_id = workspace_checkpoint_id(&db, &workspace_id)?;
    let pulse = named_pulses_for_path(&cwd)?
        .into_iter()
        .find(|pulse| pulse.id == pulse_id)
        .ok_or_else(|| "named pulse not found".to_string())?;
    let run = run_shell_command(
        &workspace_id,
        &cwd,
        &pulse.command,
        "pulse",
        &pulse.title,
        checkpoint_id.as_deref(),
    )?;
    store_terminal_run(&db, &run)?;
    Ok(run)
}

#[tauri::command]
fn start_pty_terminal(
    workspace_id: String,
    state: State<AppState>,
) -> Result<PtyTerminalInfo, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);

    let id = Uuid::new_v4().to_string();
    let started_at = now_ms();
    let session = spawn_pty_shell(&workspace_id, &cwd, started_at)?;
    session.start_output_reader()?;

    let info = PtyTerminalInfo {
        id: id.clone(),
        workspace_id: workspace_id.clone(),
        cwd,
        output: String::new(),
        is_running: true,
        started_at,
    };
    state
        .ptys
        .lock()
        .map_err(|err| err.to_string())?
        .insert(id, session);
    let db = state.db.lock().map_err(|err| err.to_string())?;
    upsert_pty_terminal_snapshot(&db, &info)?;
    Ok(info)
}

#[tauri::command]
fn list_pty_terminals(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<PtyTerminalInfo>, String> {
    let mut ptys = state.ptys.lock().map_err(|err| err.to_string())?;
    let mut terminals = Vec::new();
    let mut live_ids = Vec::new();
    for (id, session) in ptys.iter_mut() {
        if session.workspace_id() == workspace_id {
            let info = terminal_info(id, session)?;
            live_ids.push(id.clone());
            terminals.push(info);
        }
    }
    drop(ptys);

    let db = state.db.lock().map_err(|err| err.to_string())?;
    for terminal in &terminals {
        upsert_pty_terminal_snapshot(&db, terminal)?;
    }
    for snapshot in load_pty_terminal_snapshots(&db, &workspace_id)? {
        if !live_ids.iter().any(|id| id == &snapshot.id) {
            terminals.push(snapshot);
        }
    }
    terminals.sort_by_key(|terminal| terminal.started_at);
    Ok(terminals)
}

#[tauri::command]
fn write_pty_terminal(
    terminal_id: String,
    input: String,
    state: State<AppState>,
) -> Result<PtyTerminalInfo, String> {
    let mut ptys = state.ptys.lock().map_err(|err| err.to_string())?;
    let session = ptys
        .get_mut(&terminal_id)
        .ok_or_else(|| "terminal not found".to_string())?;
    session.write_input(&input)?;
    let info = terminal_info(&terminal_id, session)?;
    drop(ptys);
    let db = state.db.lock().map_err(|err| err.to_string())?;
    upsert_pty_terminal_snapshot(&db, &info)?;
    Ok(info)
}

#[tauri::command]
fn read_pty_terminal(
    terminal_id: String,
    state: State<AppState>,
) -> Result<PtyTerminalInfo, String> {
    let mut ptys = state.ptys.lock().map_err(|err| err.to_string())?;
    if let Some(session) = ptys.get_mut(&terminal_id) {
        let info = terminal_info(&terminal_id, session)?;
        drop(ptys);
        let db = state.db.lock().map_err(|err| err.to_string())?;
        upsert_pty_terminal_snapshot(&db, &info)?;
        return Ok(info);
    }
    drop(ptys);
    let db = state.db.lock().map_err(|err| err.to_string())?;
    load_pty_terminal_snapshot(&db, &terminal_id)?.ok_or_else(|| "terminal not found".to_string())
}

#[tauri::command]
fn stop_pty_terminal(
    terminal_id: String,
    state: State<AppState>,
) -> Result<PtyTerminalInfo, String> {
    let mut ptys = state.ptys.lock().map_err(|err| err.to_string())?;
    let session = ptys
        .get_mut(&terminal_id)
        .ok_or_else(|| "terminal not found".to_string())?;
    session.stop();
    let info = terminal_info(&terminal_id, session)?;
    drop(ptys);
    let db = state.db.lock().map_err(|err| err.to_string())?;
    upsert_pty_terminal_snapshot(&db, &info)?;
    Ok(info)
}

#[tauri::command]
fn close_pty_terminal(terminal_id: String, state: State<AppState>) -> Result<(), String> {
    let mut ptys = state.ptys.lock().map_err(|err| err.to_string())?;
    if let Some(mut session) = ptys.remove(&terminal_id) {
        session.stop();
    }
    drop(ptys);
    let db = state.db.lock().map_err(|err| err.to_string())?;
    delete_pty_terminal_snapshot(&db, &terminal_id)?;
    Ok(())
}

#[tauri::command]
fn start_spotlighter(
    workspace_id: String,
    state: State<AppState>,
) -> Result<SpotlighterInfo, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (workspace_path, root_path) = db
        .query_row(
            "SELECT workspaces.path, repos.path
             FROM workspaces JOIN repos ON repos.id = workspaces.repo_id
             WHERE workspaces.id = ?1",
            params![workspace_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    drop(db);

    let mut spotlighters = state.spotlighters.lock().map_err(|err| err.to_string())?;
    if let Some(existing) = spotlighters.get_mut(&workspace_id) {
        return spotlighter_info(existing);
    }

    let mut child = Command::new(spotlighter_path())
        .current_dir(&workspace_path)
        .env("LOOMEN_ROOT_PATH", &root_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("failed to start spotlighter: {err}"))?;
    if let Ok(Some(status)) = child.try_wait() {
        return Err(format!("spotlighter exited immediately: {status}"));
    }
    let process = SpotlighterProcess {
        child,
        workspace_id: workspace_id.clone(),
        workspace_path,
        root_path,
        started_at: now_ms(),
    };
    let info = spotlighter_info_ref(&process);
    spotlighters.insert(workspace_id, process);
    Ok(info)
}

#[tauri::command]
fn stop_spotlighter(
    workspace_id: String,
    state: State<AppState>,
) -> Result<SpotlighterInfo, String> {
    let mut spotlighters = state.spotlighters.lock().map_err(|err| err.to_string())?;
    let mut process = spotlighters
        .remove(&workspace_id)
        .ok_or_else(|| "spotlighter not running".to_string())?;
    let _ = process.child.kill();
    let _ = process.child.wait();
    spotlighter_info(&mut process)
}

#[tauri::command]
fn spotlighter_status(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Option<SpotlighterInfo>, String> {
    let mut spotlighters = state.spotlighters.lock().map_err(|err| err.to_string())?;
    match spotlighters.get_mut(&workspace_id) {
        Some(process) => Ok(Some(spotlighter_info(process)?)),
        None => Ok(None),
    }
}

#[tauri::command]
fn get_pull_request_info(
    workspace_id: String,
    state: State<AppState>,
) -> Result<PullRequestInfo, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);

    get_pull_request_info_for_cwd(&workspace_id, &cwd)
}

#[tauri::command]
fn create_pull_request(
    workspace_id: String,
    title: String,
    body: String,
    draft: bool,
    state: State<AppState>,
) -> Result<PullRequestInfo, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (cwd, branch_name, base_branch) = db
        .query_row(
            "SELECT path, branch_name, base_branch FROM workspaces WHERE id = ?1",
            params![workspace_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    drop(db);

    create_pull_request_for_cwd(
        &workspace_id,
        &cwd,
        branch_name,
        base_branch,
        &title,
        &body,
        draft,
    )
}

#[tauri::command]
fn update_pull_request(
    workspace_id: String,
    title: String,
    body: String,
    state: State<AppState>,
) -> Result<PullRequestInfo, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);

    update_pull_request_for_cwd(&workspace_id, &cwd, &title, &body)
}

#[tauri::command]
fn rerun_failed_checks(workspace_id: String, state: State<AppState>) -> Result<String, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (cwd, branch_name) = db
        .query_row(
            "SELECT path, branch_name FROM workspaces WHERE id = ?1",
            params![workspace_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    drop(db);

    let branch = branch_name
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| git_output(&cwd, &["branch", "--show-current"]).ok())
        .ok_or_else(|| "could not determine workspace branch".to_string())?;
    rerun_failed_checks_for_branch(&cwd, &branch)
}

#[tauri::command]
fn list_workspace_files(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<FileEntry>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    let output = git_output(
        &cwd,
        &["ls-files", "--cached", "--others", "--exclude-standard"],
    )?;
    let mut entries = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(300)
        .map(|path| {
            let name = Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path)
                .to_string();
            FileEntry {
                path: path.to_string(),
                name,
                kind: file_kind(path),
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

#[tauri::command]
fn read_workspace_file(
    workspace_id: String,
    file_path: String,
    state: State<AppState>,
) -> Result<FilePreview, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);

    let canonical = workspace_file_path(&cwd, &file_path)?;

    const MAX_PREVIEW_BYTES: usize = 220_000;
    let bytes = std::fs::read(&canonical).map_err(|err| err.to_string())?;
    let truncated = bytes.len() > MAX_PREVIEW_BYTES;
    let preview_bytes = &bytes[..bytes.len().min(MAX_PREVIEW_BYTES)];
    let is_binary = preview_bytes.contains(&0);
    let content = if is_binary {
        format!("Binary file preview unavailable ({} bytes).", bytes.len())
    } else {
        String::from_utf8_lossy(preview_bytes).to_string()
    };
    Ok(FilePreview {
        workspace_id,
        path: file_path,
        content,
        is_binary,
        truncated,
    })
}

#[tauri::command]
fn reveal_workspace_file(
    workspace_id: String,
    file_path: String,
    state: State<AppState>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);
    let canonical = workspace_file_path(&cwd, &file_path)?;
    let status = Command::new("open")
        .arg("-R")
        .arg(canonical)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open -R failed with status {status}"))
    }
}

#[tauri::command]
fn open_workspace_file_external(
    workspace_id: String,
    file_path: String,
    state: State<AppState>,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);
    let canonical = workspace_file_path(&cwd, &file_path)?;
    let status = Command::new("open")
        .arg(canonical)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open failed with status {status}"))
    }
}

#[tauri::command]
fn search_workspace(
    workspace_id: String,
    query: String,
    state: State<AppState>,
) -> Result<Vec<WorkspaceSearchMatch>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    drop(db);

    let output = Command::new("rg")
        .arg("--line-number")
        .arg("--column")
        .arg("--no-heading")
        .arg("--color")
        .arg("never")
        .arg("--fixed-strings")
        .arg("--glob")
        .arg("!.git")
        .arg("--max-count")
        .arg("200")
        .arg("--")
        .arg(query)
        .arg(".")
        .current_dir(&cwd)
        .output()
        .map_err(|err| format!("failed to run rg: {err}"))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let mut matches = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines().take(200) {
        if let Some(search_match) = parse_rg_match(line) {
            matches.push(search_match);
        }
    }
    Ok(matches)
}

#[tauri::command]
fn list_workspace_changes(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<FileEntry>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let cwd = workspace_path(&db, &workspace_id)?;
    let output = git_output(&cwd, &["status", "--short"])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let status = line[..2].trim().to_string();
            let path = line[3..].trim().to_string();
            let name = Path::new(&path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&path)
                .to_string();
            Some(FileEntry {
                path,
                name,
                kind: if status.is_empty() {
                    "changed"
                } else {
                    &status
                }
                .to_string(),
            })
        })
        .collect())
}

#[tauri::command]
fn get_workspace_patch(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<DiffFile>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let (workspace_path, checkpoint_id, base_branch) = db
        .query_row(
            "SELECT path, checkpoint_id, base_branch FROM workspaces WHERE id = ?1",
            params![workspace_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;

    let raw = match checkpoint_id.as_deref() {
        Some(id) if !id.is_empty() => checkpoint_diff(&workspace_path, id, "current"),
        _ => git_output(
            &workspace_path,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-ext-diff",
                "--unified=60",
                base_branch.as_deref().unwrap_or("HEAD"),
            ],
        ),
    }
    .unwrap_or_default();
    Ok(parse_diff_files(&raw))
}

#[tauri::command]
fn add_diff_comment(
    workspace_id: String,
    file_path: String,
    line_number: i64,
    body: String,
    state: State<AppState>,
) -> Result<Vec<DiffComment>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    add_diff_comment_for_db(&db, &workspace_id, &file_path, line_number, &body)
}

#[tauri::command]
fn resolve_diff_comment(
    comment_id: String,
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<DiffComment>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    resolve_diff_comment_for_db(&db, &comment_id, &workspace_id)
}

#[tauri::command]
fn list_diff_comments(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<DiffComment>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    list_diff_comments_for_db(&db, &workspace_id)
}

fn non_empty(value: &str, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn ensure_workspace(
    db: &Connection,
    repo_id: &str,
    name: &str,
    path: &str,
    state: &str,
    branch_name: Option<&str>,
    base_branch: Option<&str>,
    checkpoint_id: Option<&str>,
    now: i64,
) -> Result<String, String> {
    let existing = db
        .query_row(
            "SELECT id FROM workspaces WHERE repo_id = ?1 AND path = ?2",
            params![repo_id, path],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO workspaces (id, repo_id, name, path, state, branch_name, base_branch, checkpoint_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            repo_id,
            if name.is_empty() { "main" } else { name },
            path,
            state,
            branch_name,
            base_branch,
            checkpoint_id,
            now,
            now
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(id)
}

fn workspace_script(
    db: &Connection,
    workspace_id: &str,
    column: &str,
) -> Result<(String, String), String> {
    let query = format!(
        "SELECT workspaces.path, COALESCE(repos.{column}, '')
         FROM workspaces JOIN repos ON repos.id = workspaces.repo_id
         WHERE workspaces.id = ?1"
    );
    db.query_row(&query, params![workspace_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "workspace not found".to_string())
}

fn load_query_context(db: &Connection, session_id: &str) -> Result<QueryContext, String> {
    db.query_row(
        "SELECT sessions.agent_type, workspaces.path, sessions.model, sessions.permission_mode
         FROM sessions
         JOIN workspaces ON workspaces.id = sessions.workspace_id
         WHERE sessions.id = ?1",
        params![session_id],
        |row| {
            Ok(QueryContext {
                agent_type: row.get::<_, String>(0)?,
                cwd: row.get::<_, String>(1)?,
                model: row.get::<_, Option<String>>(2)?,
                permission_mode: row.get::<_, String>(3)?,
            })
        },
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "session not found".to_string())
}

fn sever_cleanup_preview_for_db(
    db: &Connection,
    workspace_id: &str,
) -> Result<SeverCleanupPreview, String> {
    let (name, state, path, branch_name, setup_log_path, run_log_path, archive_commit) = db
        .query_row(
            "SELECT name, state, path, branch_name, setup_log_path, run_log_path, archive_commit
             FROM workspaces WHERE id = ?1",
            params![workspace_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "workspace not found".to_string())?;
    let worktree_exists = Path::new(&path).exists();
    let has_setup_log = setup_log_path
        .as_deref()
        .map(|path| Path::new(path).exists())
        .unwrap_or(false);
    let has_run_log = run_log_path
        .as_deref()
        .map(|path| Path::new(path).exists())
        .unwrap_or(false);
    let branch_exists = branch_name
        .as_deref()
        .filter(|branch| !branch.trim().is_empty())
        .and_then(|branch| branch_exists_for_worktree(&path, branch));
    let session_count = workspace_count(db, "sessions", workspace_id)?;
    let terminal_run_count = workspace_count(db, "terminal_sessions", workspace_id)?;
    let terminal_tab_count = workspace_count(db, "pty_terminal_tabs", workspace_id)?;
    let diff_comment_count = workspace_count(db, "diff_comments", workspace_id)?;
    let database_record_count =
        1 + session_count + terminal_run_count + terminal_tab_count + diff_comment_count;
    let mut warnings = Vec::new();
    if state != "archived" {
        warnings.push("Archive this workspace before destructive cleanup.".to_string());
    }
    if !worktree_exists {
        warnings.push("Worktree path is already missing on disk.".to_string());
    }
    if branch_name.as_deref().unwrap_or("").trim().is_empty() {
        warnings.push("No branch is recorded for this workspace.".to_string());
    }

    Ok(SeverCleanupPreview {
        workspace_id: workspace_id.to_string(),
        workspace_name: name,
        state,
        branch_name,
        branch_exists,
        worktree_path: path,
        worktree_exists,
        setup_log_path,
        run_log_path,
        has_setup_log,
        has_run_log,
        archive_commit,
        session_count,
        terminal_run_count,
        terminal_tab_count,
        diff_comment_count,
        database_record_count,
        warnings,
    })
}

fn workspace_count(db: &Connection, table: &str, workspace_id: &str) -> Result<i64, String> {
    db.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE workspace_id = ?1"),
        params![workspace_id],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|err| err.to_string())
}

fn write_lifecycle_log(workspace_id: &str, kind: &str, output: &str) -> Result<String, String> {
    let dir = std::env::temp_dir().join("loomen-lifecycle");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(format!("{workspace_id}-{kind}-{}.log", now_ms()));
    std::fs::write(&path, output).map_err(|err| err.to_string())?;
    Ok(path.display().to_string())
}

fn setup_state_for_exit(exit_code: Option<i32>) -> &'static str {
    if exit_code == Some(0) {
        "ready"
    } else {
        "setup-failed"
    }
}

fn insert_message(
    db: &Connection,
    session_id: &str,
    role: &str,
    content: &str,
    created_at: i64,
) -> Result<(), String> {
    db.execute(
        "INSERT INTO session_messages (id, session_id, role, content, raw_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            Uuid::new_v4().to_string(),
            session_id,
            role,
            content,
            serde_json::json!({ "role": role, "content": content }).to_string(),
            created_at
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn context_usage_for_db(db: &Connection, session_id: &str) -> Result<ContextUsage, String> {
    let (agent_type, model) = db
        .query_row(
            "SELECT agent_type, model FROM sessions WHERE id = ?1",
            params![session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "session not found".to_string())?;
    let total_chars = db
        .query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM session_messages WHERE session_id = ?1",
            params![session_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| err.to_string())?;
    let used_tokens = estimate_tokens(total_chars);
    let max_tokens = context_limit_for_model(&agent_type, model.as_deref());
    let percent = if max_tokens <= 0 {
        0.0
    } else {
        ((used_tokens as f64 / max_tokens as f64) * 100.0).min(100.0)
    };
    Ok(ContextUsage {
        session_id: session_id.to_string(),
        used_tokens,
        max_tokens,
        percent,
    })
}

fn estimate_tokens(chars: i64) -> i64 {
    ((chars.max(0) + 3) / 4).max(0)
}

fn context_limit_for_model(agent_type: &str, model: Option<&str>) -> i64 {
    let model = model.unwrap_or_default();
    if agent_type == "codex" {
        if model.contains("gpt-5") {
            272_000
        } else {
            128_000
        }
    } else if model.contains("haiku") {
        200_000
    } else if model.contains("sonnet") || model.contains("opus") || model.is_empty() {
        200_000
    } else {
        128_000
    }
}

fn repo_workspace_context(
    db: &Connection,
    repo_id: &str,
) -> Result<(String, String, Option<String>, Option<String>), String> {
    db.query_row(
        "SELECT path, name, current_branch, default_branch FROM repos WHERE id = ?1",
        params![repo_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        },
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "repo not found".to_string())
}

fn build_workspace_plan(
    repo_path: &str,
    repo_name: &str,
    current_branch: Option<&str>,
    default_branch: Option<&str>,
    settings: &AppSettings,
    name: &str,
    path: &str,
    base_branch: Option<&str>,
    suffix: Option<&str>,
) -> WorkspacePlan {
    let workspace_name = non_empty(name.trim(), "workspace");
    let suffix = suffix
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string()[..8].to_string());
    let branch_leaf = format!("{}-{}", slugify(&workspace_name), suffix);
    let branch_name = workspace_branch_name(repo_path, settings, &branch_leaf);
    let requested_base = base_branch.map(str::trim).filter(|value| !value.is_empty());
    let base_branch = requested_base
        .or(current_branch)
        .or(default_branch)
        .unwrap_or("HEAD")
        .to_string();
    let worktree_path = if path.trim().is_empty() {
        default_worktree_path(settings, repo_name, &branch_leaf)
    } else {
        expand_tilde(path.trim())
    };
    let checkpoint_id = format!("workspace-{suffix}");
    let path_ref = Path::new(&worktree_path);
    let path_exists = path_ref.exists();
    let path_is_empty = if path_exists {
        path_ref
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false)
    } else {
        true
    };
    let mut warnings = Vec::new();
    if path_exists && !path_is_empty {
        warnings.push(format!("worktree path is not empty: {worktree_path}"));
    }
    WorkspacePlan {
        workspace_name,
        branch_leaf,
        branch_name,
        base_branch,
        worktree_path,
        checkpoint_id,
        path_exists,
        path_is_empty,
        can_create: warnings.is_empty(),
        warnings,
    }
}

fn workspace_branch_name(repo_path: &str, settings: &AppSettings, branch_leaf: &str) -> String {
    let prefix = match settings.branch_prefix_type.as_str() {
        "none" => String::new(),
        "custom" => slugify(&settings.branch_prefix_custom),
        _ => detect_branch_user_prefix(repo_path).unwrap_or_else(|| "loomen".to_string()),
    };
    if prefix.is_empty() {
        branch_leaf.to_string()
    } else {
        format!("{}/{}", prefix.trim_matches('/'), branch_leaf)
    }
}

fn detect_branch_user_prefix(repo_path: &str) -> Option<String> {
    git_output(repo_path, &["config", "--get", "github.user"])
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("USER")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .map(|value| slugify(&value))
        .filter(|value| !value.is_empty())
}

fn default_worktree_path(settings: &AppSettings, repo_name: &str, branch_leaf: &str) -> String {
    let root = non_empty(&settings.loomen_root_directory, "~/loomen");
    PathBuf::from(expand_tilde(&root))
        .join(slugify(repo_name))
        .join(branch_leaf)
        .display()
        .to_string()
}

fn workspace_path(db: &Connection, workspace_id: &str) -> Result<String, String> {
    db.query_row(
        "SELECT path FROM workspaces WHERE id = ?1",
        params![workspace_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "workspace not found".to_string())
}

fn workspace_checkpoint_id(db: &Connection, workspace_id: &str) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT checkpoint_id FROM workspaces WHERE id = ?1",
        params![workspace_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "workspace not found".to_string())
}

fn workspace_file_path(cwd: &str, file_path: &str) -> Result<PathBuf, String> {
    if file_path.trim().is_empty()
        || Path::new(file_path).is_absolute()
        || Path::new(file_path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("invalid workspace file path".to_string());
    }
    let root = PathBuf::from(cwd)
        .canonicalize()
        .map_err(|err| err.to_string())?;
    let target = root.join(file_path);
    let canonical = target.canonicalize().map_err(|err| err.to_string())?;
    if !canonical.starts_with(&root) {
        return Err("file is outside the workspace".to_string());
    }
    if !canonical.is_file() {
        return Err("workspace path is not a file".to_string());
    }
    Ok(canonical)
}

fn parse_rg_match(line: &str) -> Option<WorkspaceSearchMatch> {
    let mut parts = line.splitn(4, ':');
    let path = parts.next()?.trim_start_matches("./").to_string();
    let line_number = parts.next()?.parse::<usize>().ok()?;
    let column = parts.next()?.parse::<usize>().ok()?;
    let text = parts.next().unwrap_or_default().to_string();
    Some(WorkspaceSearchMatch {
        path,
        line: line_number,
        column,
        text,
    })
}

fn session_workspace_id(db: &Connection, session_id: &str) -> Result<String, String> {
    db.query_row(
        "SELECT workspace_id FROM sessions WHERE id = ?1",
        params![session_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "session not found".to_string())
}

fn workspace_diff_basis(
    db: &Connection,
    workspace_id: &str,
) -> Result<(String, Option<String>, Option<String>), String> {
    db.query_row(
        "SELECT path, checkpoint_id, base_branch FROM workspaces WHERE id = ?1",
        params![workspace_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )
    .optional()
    .map_err(|err| err.to_string())?
    .ok_or_else(|| "workspace not found".to_string())
}

fn open_path_in_finder(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    let status = Command::new("open")
        .arg(path)
        .status()
        .map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open failed with status {status}"))
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "workspace".to_string()
    } else {
        slug.chars().take(48).collect()
    }
}

fn file_kind(path: &str) -> String {
    if path.ends_with('/') {
        "folder".to_string()
    } else if path == ".git" || path.starts_with(".git/") {
        "git".to_string()
    } else if path.ends_with(".md") {
        "markdown".to_string()
    } else if path.ends_with(".json") {
        "json".to_string()
    } else if path.ends_with(".lock") {
        "lock".to_string()
    } else {
        "file".to_string()
    }
}

fn launch_health_status(checks: &[LaunchHealthCheck]) -> String {
    if checks.iter().any(|check| check.status == "error") {
        "error".to_string()
    } else if checks.iter().any(|check| check.status == "warning") {
        "warning".to_string()
    } else {
        "ok".to_string()
    }
}

fn path_health_check(
    id: &str,
    label: &str,
    path: &Path,
    required: bool,
    ok: bool,
    ok_detail: &str,
    error_detail: &str,
    remediation: &str,
) -> LaunchHealthCheck {
    LaunchHealthCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: if ok { "ok" } else { "error" }.to_string(),
        detail: if ok { ok_detail } else { error_detail }.to_string(),
        path: Some(path.display().to_string()),
        version: None,
        required,
        remediation: if ok {
            None
        } else {
            Some(remediation.to_string())
        },
    }
}

fn command_health_check(
    id: &str,
    label: &str,
    command: &str,
    version_args: &[&str],
    required: bool,
    remediation: &str,
) -> LaunchHealthCheck {
    let path_value = std::env::var_os("PATH").and_then(|value| value.into_string().ok());
    let resolved = resolve_launch_executable("", None, path_value.as_deref(), command);
    executable_health_check(
        id,
        label,
        command,
        required,
        remediation,
        resolved,
        version_args,
    )
}

fn agent_health_check(
    id: &str,
    label: &str,
    command: &str,
    configured_path: &str,
    env_key: &str,
    remediation: &str,
) -> LaunchHealthCheck {
    let env_value = std::env::var(env_key).ok();
    let path_value = std::env::var_os("PATH").and_then(|value| value.into_string().ok());
    let resolved = resolve_launch_executable(
        configured_path,
        env_value.as_deref(),
        path_value.as_deref(),
        command,
    );
    executable_health_check(id, label, command, false, remediation, resolved, &[])
}

fn executable_health_check(
    id: &str,
    label: &str,
    command: &str,
    required: bool,
    remediation: &str,
    resolved: LaunchExecutable,
    version_args: &[&str],
) -> LaunchHealthCheck {
    let status = if resolved.problem.is_some() {
        if required {
            "error"
        } else {
            "warning"
        }
    } else {
        "ok"
    };
    let version = if status == "ok" {
        resolved
            .path
            .as_deref()
            .and_then(|path| command_version(path, version_args))
    } else {
        None
    };
    LaunchHealthCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail: resolved.problem.unwrap_or_else(|| {
            format!(
                "{} resolved from {}.",
                command,
                resolved.source.to_lowercase()
            )
        }),
        path: resolved.path,
        version,
        required,
        remediation: if status == "ok" {
            None
        } else {
            Some(remediation.to_string())
        },
    }
}

fn resolve_launch_executable(
    configured_path: &str,
    env_value: Option<&str>,
    path_env: Option<&str>,
    command: &str,
) -> LaunchExecutable {
    let mut checked_candidates = Vec::new();
    let mut problems = Vec::new();

    if let Some(result) = candidate_launch_executable(
        "settings",
        Some(configured_path),
        &mut checked_candidates,
        &mut problems,
    ) {
        return result;
    }
    if let Some(result) = candidate_launch_executable(
        "environment",
        env_value,
        &mut checked_candidates,
        &mut problems,
    ) {
        return result;
    }
    if let Some(path) = find_executable_in_path(command, path_env) {
        checked_candidates.push(format!("PATH:{}", path.display()));
        return LaunchExecutable {
            path: Some(path.display().to_string()),
            source: "PATH".to_string(),
            problem: None,
            checked_candidates,
        };
    }
    problems.push(format!("{command} not found on PATH"));
    LaunchExecutable {
        path: None,
        source: "PATH".to_string(),
        problem: Some(problems.join("; ")),
        checked_candidates,
    }
}

fn candidate_launch_executable(
    source: &str,
    value: Option<&str>,
    checked_candidates: &mut Vec<String>,
    problems: &mut Vec<String>,
) -> Option<LaunchExecutable> {
    let value = value.map(str::trim).filter(|value| !value.is_empty())?;
    let path = PathBuf::from(expand_tilde(value));
    checked_candidates.push(format!("{source}:{}", path.display()));
    if !path.exists() {
        problems.push(format!("{} from {source} does not exist", path.display()));
        return None;
    }
    if !is_executable_file(&path) {
        return Some(LaunchExecutable {
            path: Some(path.display().to_string()),
            source: source.to_string(),
            problem: Some(format!(
                "{} from {source} is not executable",
                path.display()
            )),
            checked_candidates: checked_candidates.clone(),
        });
    }
    Some(LaunchExecutable {
        path: Some(path.display().to_string()),
        source: source.to_string(),
        problem: None,
        checked_candidates: checked_candidates.clone(),
    })
}

fn find_executable_in_path(command: &str, path_env: Option<&str>) -> Option<PathBuf> {
    let path_env = path_env?;
    std::env::split_paths(path_env).find_map(|dir| {
        let candidate = dir.join(command);
        if is_executable_file(&candidate) {
            Some(candidate)
        } else {
            None
        }
    })
}

fn is_executable_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn command_version(path: &str, args: &[&str]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let output = Command::new(path).args(args).output().ok()?;
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr)
    } else {
        String::from_utf8_lossy(&output.stdout)
    };
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn sidecar_launch_health(sidecar: &mut Option<SidecarProcess>) -> LaunchHealthCheck {
    let health = sidecar_health(sidecar);
    LaunchHealthCheck {
        id: "sidecar".to_string(),
        label: "Sidecar".to_string(),
        status: health.status.to_string(),
        detail: health.detail,
        path: health.path,
        version: None,
        required: true,
        remediation: health.remediation,
    }
}

fn spotlighter_path() -> PathBuf {
    rebuild_root().join("script").join("spotlighter.sh")
}

fn spotlighter_info(process: &mut SpotlighterProcess) -> Result<SpotlighterInfo, String> {
    let is_running = match process.child.try_wait() {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(_) => false,
    };
    Ok(SpotlighterInfo {
        workspace_id: process.workspace_id.clone(),
        workspace_path: process.workspace_path.clone(),
        root_path: process.root_path.clone(),
        is_running,
        started_at: process.started_at,
    })
}

fn spotlighter_info_ref(process: &SpotlighterProcess) -> SpotlighterInfo {
    SpotlighterInfo {
        workspace_id: process.workspace_id.clone(),
        workspace_path: process.workspace_path.clone(),
        root_path: process.root_path.clone(),
        is_running: true,
        started_at: process.started_at,
    }
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest).display().to_string();
        }
    }
    path.to_string()
}

fn path_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn title_from_prompt(prompt: &str) -> String {
    let compact = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        "Untitled".to_string()
    } else {
        compact.chars().take(48).collect()
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn open_database(app: &tauri::App) -> anyhow::Result<(Connection, PathBuf)> {
    let app_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&app_dir)?;
    let db_path = app_dir.join("loomen.db");
    let db = Connection::open(&db_path)?;
    init_db(&db)?;
    Ok((db, db_path))
}

fn send_query_to_sidecar(
    state: &State<AppState>,
    session_id: &str,
    prompt: &str,
    agent_type: &str,
    cwd: &str,
    model: Option<&str>,
    permission_mode: &str,
) -> Result<String, String> {
    send_query_to_sidecar_streaming(
        state,
        None,
        session_id,
        prompt,
        agent_type,
        cwd,
        model,
        permission_mode,
        |_| {},
        |_| {},
    )
}

fn sidecar_rpc(
    state: &State<AppState>,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let socket_path = {
        let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
        sidecar_socket_path(&state.rebuild_root, &mut sidecar)?
    };
    sidecar_socket_rpc(&socket_path, method, params, timeout)
}

fn sidecar_runtime_settings(settings: &AppSettings) -> SidecarRuntimeSettings {
    SidecarRuntimeSettings {
        provider_env: settings.provider_env.clone(),
        codex_provider_mode: settings.codex_provider_mode.clone(),
        default_codex_effort: settings.default_codex_effort.clone(),
        codex_personality: settings.codex_personality.clone(),
        claude_executable_path: settings.claude_executable_path.clone(),
        codex_executable_path: settings.codex_executable_path.clone(),
        claude_tool_approvals: settings.claude_tool_approvals,
    }
}

fn send_query_to_sidecar_streaming<F, G>(
    state: &State<AppState>,
    app_handle: Option<&AppHandle>,
    session_id: &str,
    prompt: &str,
    agent_type: &str,
    cwd: &str,
    model: Option<&str>,
    permission_mode: &str,
    mut on_message: F,
    mut on_session_event: G,
) -> Result<String, String>
where
    F: FnMut(String),
    G: FnMut(Value),
{
    let settings = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        load_settings(&db).map_err(|err| err.to_string())?
    };
    let runtime_settings = sidecar_runtime_settings(&settings);
    let socket_path = {
        let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
        sidecar_socket_path(&state.rebuild_root, &mut sidecar)?
    };
    send_query_to_sidecar_socket(
        &socket_path,
        &runtime_settings,
        SidecarQuery {
            session_id,
            prompt,
            agent_type,
            cwd,
            model,
            permission_mode,
        },
        |request| handle_reverse_rpc(state, app_handle, request, session_id),
        &mut on_message,
        &mut on_session_event,
    )
}

fn handle_reverse_rpc(
    state: &State<AppState>,
    app_handle: Option<&AppHandle>,
    request: &Value,
    session_id: &str,
) -> Result<Value, String> {
    if request.get("method").and_then(Value::as_str) == Some("toolApproval") {
        return Ok(reverse_tool_approval_interactive(
            state, app_handle, request, session_id,
        ));
    }
    let db = state.db.lock().map_err(|err| err.to_string())?;
    Ok(reverse_rpc_response(&db, request, session_id))
}

fn reverse_rpc_response(db: &Connection, request: &Value, session_id: &str) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "getDiff" => reverse_get_diff(db, session_id),
        "diffComment" => reverse_diff_comment(db, request, session_id),
        "getTerminalOutput" => reverse_get_terminal_output(db, session_id),
        "toolApproval" => reverse_tool_approval_for_db(db, session_id),
        "askUserQuestion" => Ok(serde_json::json!({
            "answer": null,
            "skipped": true,
            "reason": "interactive question UI is not implemented in loomen yet"
        })),
        "exitPlanMode" => Ok(serde_json::json!({ "ok": true })),
        _ => {
            return serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            });
        }
    };

    match result {
        Ok(result) => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(message) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32000, "message": message }
        }),
    }
}

fn json_rpc_result(id: Value, result: Value) -> Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn json_rpc_error(id: Value, code: i64, message: String) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn reverse_get_diff(db: &Connection, session_id: &str) -> Result<Value, String> {
    let workspace_id = session_workspace_id(db, session_id)?;
    let (workspace_path, checkpoint_id, base_branch) = workspace_diff_basis(db, &workspace_id)?;
    let raw = match checkpoint_id.as_deref() {
        Some(id) if !id.is_empty() => checkpoint_diff(&workspace_path, id, "current"),
        _ => git_output(
            &workspace_path,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-ext-diff",
                "--unified=60",
                base_branch.as_deref().unwrap_or("HEAD"),
            ],
        ),
    }
    .unwrap_or_default();
    Ok(serde_json::json!({
        "workspaceId": workspace_id,
        "diff": raw,
        "files": parse_diff_files(&raw)
    }))
}

fn reverse_diff_comment(
    db: &Connection,
    request: &Value,
    session_id: &str,
) -> Result<Value, String> {
    let workspace_id = session_workspace_id(db, session_id)?;
    let params = request.get("params").unwrap_or(&Value::Null);
    let comments = add_diff_comment_from_params(db, &workspace_id, params)?;
    Ok(serde_json::json!({
        "workspaceId": workspace_id,
        "comments": comments
    }))
}

fn reverse_get_terminal_output(db: &Connection, session_id: &str) -> Result<Value, String> {
    let workspace_id = session_workspace_id(db, session_id)?;
    let output = db
        .query_row(
            "SELECT output FROM terminal_sessions WHERE workspace_id = ?1 ORDER BY ended_at DESC LIMIT 1",
            params![workspace_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .unwrap_or_default();
    Ok(serde_json::json!({
        "workspaceId": workspace_id,
        "output": output
    }))
}

fn reverse_tool_approval_for_db(db: &Connection, session_id: &str) -> Result<Value, String> {
    let permission_mode = db
        .query_row(
            "SELECT permission_mode FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| "default".to_string());
    let (approved, reason) = tool_approval_for_permission(&permission_mode);
    Ok(serde_json::json!({
        "approved": approved,
        "permissionMode": permission_mode,
        "reason": reason
    }))
}

fn reverse_tool_approval_interactive(
    state: &State<AppState>,
    app_handle: Option<&AppHandle>,
    request: &Value,
    session_id: &str,
) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let permission_mode = {
        let db = match state.db.lock() {
            Ok(db) => db,
            Err(err) => return json_rpc_error(id, -32000, err.to_string()),
        };
        match db
            .query_row(
                "SELECT permission_mode FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
        {
            Ok(Some(value)) => value,
            Ok(None) => "default".to_string(),
            Err(err) => return json_rpc_error(id, -32000, err.to_string()),
        }
    };
    let (auto_approved, reason) = tool_approval_for_permission(&permission_mode);
    if auto_approved {
        return json_rpc_result(
            id,
            serde_json::json!({
                "approved": true,
                "permissionMode": permission_mode,
                "reason": reason
            }),
        );
    }

    let Some(app_handle) = app_handle else {
        return json_rpc_result(
            id,
            serde_json::json!({
                "approved": false,
                "permissionMode": permission_mode,
                "reason": "interactive approval is unavailable for this request"
            }),
        );
    };

    let approval_id = Uuid::new_v4().to_string();
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let tool_name = params
        .get("toolName")
        .or_else(|| params.get("tool_name"))
        .or_else(|| params.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let input = params
        .get("input")
        .or_else(|| params.get("arguments"))
        .cloned()
        .unwrap_or(params);
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut approvals) = state.approvals.lock() {
        approvals.insert(approval_id.clone(), sender);
    } else {
        return json_rpc_error(id, -32000, "approval queue is unavailable".to_string());
    }

    let event = ToolApprovalRequest {
        approval_id: approval_id.clone(),
        session_id: session_id.to_string(),
        tool_name,
        input,
        permission_mode: permission_mode.clone(),
        requested_at: now_ms(),
    };
    if let Err(err) = app_handle.emit("loomen-tool-approval-requested", event) {
        let _ = state
            .approvals
            .lock()
            .map(|mut approvals| approvals.remove(&approval_id));
        return json_rpc_error(id, -32000, err.to_string());
    }

    match receiver.recv_timeout(Duration::from_secs(60 * 5)) {
        Ok(decision) => json_rpc_result(
            id,
            serde_json::json!({
                "approved": decision.approved,
                "permissionMode": permission_mode,
                "reason": if decision.approved { "approved by user" } else { "rejected by user" }
            }),
        ),
        Err(_) => {
            let _ = state
                .approvals
                .lock()
                .map(|mut approvals| approvals.remove(&approval_id));
            json_rpc_result(
                id,
                serde_json::json!({
                    "approved": false,
                    "permissionMode": permission_mode,
                    "reason": "approval request timed out"
                }),
            )
        }
    }
}

fn tool_approval_for_permission(permission_mode: &str) -> (bool, &'static str) {
    if matches!(
        permission_mode,
        "acceptEdits" | "auto" | "bypassPermissions" | "dontAsk"
    ) {
        (true, "approved by session permission mode")
    } else {
        (
            false,
            "interactive approval is required for this permission mode",
        )
    }
}

fn rebuild_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent rebuild root")
        .to_path_buf()
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let (db, db_path) = open_database(app).map_err(|err| err.to_string())?;
            app.manage(AppState {
                db: Mutex::new(db),
                db_path,
                rebuild_root: rebuild_root(),
                sidecar: Mutex::new(None),
                ptys: Mutex::new(HashMap::new()),
                spotlighters: Mutex::new(HashMap::new()),
                approvals: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_settings,
            update_settings,
            open_workspace_in_finder,
            open_repo_in_finder,
            add_repo,
            preview_workspace,
            create_workspace,
            create_session,
            close_session,
            send_query,
            start_query,
            update_session_settings,
            get_context_usage,
            resolve_tool_approval,
            get_db_path,
            get_launch_health,
            sidecar_status,
            workspace_init,
            claude_auth_status,
            cancel_query,
            update_workspace_notes,
            update_repo_scripts,
            run_workspace_setup,
            run_workspace_run_script,
            archive_workspace,
            restore_workspace,
            preview_sever_cleanup,
            save_workspace_checkpoint,
            get_workspace_diff,
            run_terminal_command,
            list_pulse_evidence,
            list_named_pulses,
            run_named_pulse,
            list_workspace_files,
            read_workspace_file,
            reveal_workspace_file,
            open_workspace_file_external,
            search_workspace,
            list_workspace_changes,
            get_workspace_patch,
            add_diff_comment,
            resolve_diff_comment,
            list_diff_comments,
            start_pty_terminal,
            list_pty_terminals,
            write_pty_terminal,
            read_pty_terminal,
            stop_pty_terminal,
            close_pty_terminal,
            start_spotlighter,
            stop_spotlighter,
            spotlighter_status,
            get_pull_request_info,
            create_pull_request,
            update_pull_request,
            rerun_failed_checks
        ])
        .run(tauri::generate_context!())
        .expect("error while running Loomen");
}

fn main() {
    run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn resolves_launch_executable_precedence_and_path_search(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_root = std::env::temp_dir().join(format!("loomen-health-{}", Uuid::new_v4()));
        let bin_dir = temp_root.join("bin");
        std::fs::create_dir_all(&bin_dir)?;
        let codex = bin_dir.join("codex");
        std::fs::write(&codex, "#!/bin/sh\necho codex\n")?;
        let mut permissions = std::fs::metadata(&codex)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&codex, permissions)?;
        let stale_settings = temp_root.join("missing-codex");

        let from_settings = resolve_launch_executable(
            codex.to_str().unwrap(),
            Some("/missing/env/codex"),
            Some(""),
            "codex",
        );
        assert_eq!(from_settings.source, "settings");
        assert_eq!(from_settings.path.as_deref(), codex.to_str());
        assert!(from_settings.problem.is_none());

        let from_env = resolve_launch_executable(
            stale_settings.to_str().unwrap(),
            Some(codex.to_str().unwrap()),
            Some(""),
            "codex",
        );
        assert_eq!(from_env.source, "environment");
        assert_eq!(from_env.path.as_deref(), codex.to_str());
        assert!(from_env
            .checked_candidates
            .iter()
            .any(|candidate| candidate.starts_with("settings:")));

        let from_path =
            resolve_launch_executable("", None, Some(bin_dir.to_str().unwrap()), "codex");
        assert_eq!(from_path.source, "PATH");
        assert_eq!(from_path.path.as_deref(), codex.to_str());

        let missing = resolve_launch_executable("", None, Some(""), "codex");
        assert_eq!(missing.source, "PATH");
        assert!(missing.path.is_none());
        assert!(missing.problem.unwrap().contains("not found"));

        let _ = std::fs::remove_dir_all(&temp_root);
        Ok(())
    }

    #[test]
    fn launch_executable_blocks_non_executable_existing_override(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_root = std::env::temp_dir().join(format!("loomen-health-{}", Uuid::new_v4()));
        let bin_dir = temp_root.join("bin");
        std::fs::create_dir_all(&bin_dir)?;
        let non_executable = temp_root.join("codex");
        std::fs::write(&non_executable, "#!/bin/sh\necho blocked\n")?;
        let fallback = bin_dir.join("codex");
        std::fs::write(&fallback, "#!/bin/sh\necho fallback\n")?;
        let mut permissions = std::fs::metadata(&fallback)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fallback, permissions)?;

        let resolved = resolve_launch_executable(
            non_executable.to_str().unwrap(),
            Some(fallback.to_str().unwrap()),
            Some(bin_dir.to_str().unwrap()),
            "codex",
        );
        assert_eq!(resolved.source, "settings");
        assert_eq!(resolved.path.as_deref(), non_executable.to_str());
        assert!(resolved.problem.unwrap().contains("not executable"));

        let _ = std::fs::remove_dir_all(&temp_root);
        Ok(())
    }

    #[test]
    fn sidecar_idle_is_a_healthy_lazy_start_state() {
        let mut sidecar = None;
        let check = sidecar_launch_health(&mut sidecar);
        assert_eq!(check.status, "ok");
        assert!(check.detail.contains("idle"));
        assert!(check.remediation.is_none());
    }

    #[test]
    fn launch_health_status_prefers_errors_then_warnings() {
        let mut checks = vec![LaunchHealthCheck {
            id: "git".to_string(),
            label: "Git".to_string(),
            status: "ok".to_string(),
            detail: "available".to_string(),
            path: None,
            version: None,
            required: true,
            remediation: None,
        }];
        assert_eq!(launch_health_status(&checks), "ok");

        checks.push(LaunchHealthCheck {
            id: "claude".to_string(),
            label: "Claude Code".to_string(),
            status: "warning".to_string(),
            detail: "not configured".to_string(),
            path: None,
            version: None,
            required: false,
            remediation: Some("Set a path in Advanced settings.".to_string()),
        });
        assert_eq!(launch_health_status(&checks), "warning");

        checks.push(LaunchHealthCheck {
            id: "bun".to_string(),
            label: "Bun".to_string(),
            status: "error".to_string(),
            detail: "not found".to_string(),
            path: None,
            version: None,
            required: true,
            remediation: Some("Install Bun and keep it on PATH.".to_string()),
        });
        assert_eq!(launch_health_status(&checks), "error");
    }

    #[test]
    fn parses_rg_search_match() {
        let item = parse_rg_match("./src/main.rs:42:7:fn search_workspace()").unwrap();
        assert_eq!(item.path, "src/main.rs");
        assert_eq!(item.line, 42);
        assert_eq!(item.column, 7);
        assert_eq!(item.text, "fn search_workspace()");
    }

    #[test]
    fn lifecycle_scripts_write_logs_and_terminal_records() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let now = now_ms();
        let cwd = std::env::temp_dir().display().to_string();
        db.execute(
            "INSERT INTO repos (id, name, path, setup_script, run_script, run_script_mode, created_at, updated_at)
             VALUES ('repo-1', 'repo', ?1, 'echo SETUP_OK', 'echo RUN_OK', 'concurrent', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'workspace', ?1, 'active', ?2, ?2)",
            params![cwd, now],
        )?;

        let (setup_cwd, setup_script) = workspace_script(&db, "workspace-1", "setup_script")?;
        assert_eq!(setup_cwd, cwd);
        assert_eq!(setup_script, "echo SETUP_OK");
        let setup_run = run_shell_command(
            "workspace-1",
            &setup_cwd,
            &setup_script,
            "setup",
            "Setup script",
            None,
        )?;
        assert_eq!(setup_run.exit_code, Some(0));
        assert!(setup_run.output.contains("SETUP_OK"));
        let setup_log = write_lifecycle_log("workspace-1", "setup", &setup_run.output)?;
        assert!(std::fs::read_to_string(setup_log)?.contains("SETUP_OK"));
        store_terminal_run(&db, &setup_run)?;

        let (_, run_script) = workspace_script(&db, "workspace-1", "run_script")?;
        let run = run_shell_command("workspace-1", &cwd, &run_script, "run", "Run script", None)?;
        assert_eq!(run.exit_code, Some(0));
        assert!(run.output.contains("RUN_OK"));
        store_terminal_run(&db, &run)?;

        let count: i64 = db.query_row("SELECT COUNT(*) FROM terminal_sessions", [], |row| {
            row.get(0)
        })?;
        assert_eq!(count, 2);
        Ok(())
    }

    #[test]
    fn pulse_evidence_keeps_kind_label_checkpoint_and_recency(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let now = now_ms();
        let cwd = std::env::temp_dir().display().to_string();
        db.execute(
            "INSERT INTO repos (id, name, path, setup_script, run_script, run_script_mode, created_at, updated_at)
             VALUES ('repo-1', 'repo', ?1, 'echo SETUP_OK', 'echo RUN_OK', 'concurrent', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, checkpoint_id, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'workspace', ?1, 'active', 'checkpoint-1', ?2, ?2)",
            params![cwd, now],
        )?;

        let setup = run_shell_command(
            "workspace-1",
            &cwd,
            "echo SETUP_OK",
            "setup",
            "Setup script",
            Some("checkpoint-1"),
        )?;
        store_terminal_run(&db, &setup)?;
        std::thread::sleep(Duration::from_millis(2));
        let run = run_shell_command(
            "workspace-1",
            &cwd,
            "echo RUN_OK",
            "run",
            "Run script",
            Some("checkpoint-1"),
        )?;
        store_terminal_run(&db, &run)?;

        let evidence = list_pulse_evidence_for_db(&db, "workspace-1", 10)?;
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].kind, "run");
        assert_eq!(evidence[0].label, "Run script");
        assert_eq!(evidence[0].checkpoint_id.as_deref(), Some("checkpoint-1"));
        assert!(evidence[0].duration_ms >= 0);
        assert!(evidence[0].output.contains("RUN_OK"));
        assert_eq!(evidence[1].kind, "setup");
        Ok(())
    }

    #[test]
    fn sever_cleanup_preview_counts_workspace_artifacts() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let now = now_ms();
        let root = std::env::temp_dir().join(format!("loomen-sever-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root)?;
        let setup_log = root.join("setup.log");
        std::fs::write(&setup_log, "setup")?;
        db.execute(
            "INSERT INTO repos (id, name, path, created_at, updated_at)
             VALUES ('repo-1', 'repo', ?1, ?2, ?2)",
            params![root.display().to_string(), now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, branch_name, setup_log_path, run_log_path, archive_commit, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'cleanup-target', ?1, 'archived', 'codex/demo', ?2, ?3, 'abc123', ?4, ?4)",
            params![
                root.display().to_string(),
                setup_log.display().to_string(),
                root.join("missing-run.log").display().to_string(),
                now
            ],
        )?;
        db.execute(
            "INSERT INTO sessions (id, workspace_id, title, created_at, updated_at)
             VALUES ('session-1', 'workspace-1', 'session', ?1, ?1)",
            params![now],
        )?;
        db.execute(
            "INSERT INTO terminal_sessions (id, workspace_id, command, cwd, output, exit_code, started_at, ended_at)
             VALUES ('terminal-1', 'workspace-1', 'echo ok', ?1, 'ok', 0, ?2, ?2)",
            params![root.display().to_string(), now],
        )?;
        db.execute(
            "INSERT INTO pty_terminal_tabs (id, workspace_id, cwd, output, is_running, started_at, updated_at)
             VALUES ('pty-1', 'workspace-1', ?1, 'scrollback', 0, ?2, ?2)",
            params![root.display().to_string(), now],
        )?;
        db.execute(
            "INSERT INTO diff_comments (id, workspace_id, file_path, line_number, body, created_at)
             VALUES ('comment-1', 'workspace-1', 'README.md', 1, 'check', ?1)",
            params![now],
        )?;

        let preview = sever_cleanup_preview_for_db(&db, "workspace-1")?;
        assert_eq!(preview.workspace_name, "cleanup-target");
        assert_eq!(preview.branch_name.as_deref(), Some("codex/demo"));
        assert!(preview.worktree_exists);
        assert!(preview.has_setup_log);
        assert!(!preview.has_run_log);
        assert_eq!(preview.session_count, 1);
        assert_eq!(preview.terminal_run_count, 1);
        assert_eq!(preview.terminal_tab_count, 1);
        assert_eq!(preview.diff_comment_count, 1);
        assert_eq!(preview.database_record_count, 5);
        assert!(preview.warnings.is_empty());
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn failed_setup_uses_failed_state_name() {
        assert_eq!(setup_state_for_exit(Some(0)), "ready");
        assert_eq!(setup_state_for_exit(Some(42)), "setup-failed");
        assert_eq!(setup_state_for_exit(None), "setup-failed");
    }

    #[test]
    fn reverse_rpc_handles_diff_comments_and_terminal_output(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let now = now_ms();
        let cwd = std::env::temp_dir().display().to_string();
        db.execute(
            "INSERT INTO repos (id, name, path, setup_script, run_script, run_script_mode, created_at, updated_at)
             VALUES ('repo-1', 'repo', ?1, '', '', 'concurrent', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'workspace', ?1, 'active', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO sessions (id, workspace_id, title, agent_type, permission_mode, created_at, updated_at)
             VALUES ('session-1', 'workspace-1', 'session', 'claude', 'default', ?1, ?1)",
            params![now],
        )?;
        db.execute(
            "INSERT INTO terminal_sessions (id, workspace_id, command, cwd, output, exit_code, started_at, ended_at)
             VALUES ('terminal-1', 'workspace-1', 'echo ok', ?1, 'TERMINAL_OK', 0, ?2, ?2)",
            params![cwd, now],
        )?;

        let terminal = reverse_rpc_response(
            &db,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getTerminalOutput"
            }),
            "session-1",
        );
        assert_eq!(terminal["result"]["output"], "TERMINAL_OK");

        let comment = reverse_rpc_response(
            &db,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "diffComment",
                "params": {
                    "filePath": "README.md",
                    "lineNumber": 7,
                    "body": "check this"
                }
            }),
            "session-1",
        );
        assert_eq!(comment["result"]["comments"][0]["filePath"], "README.md");
        assert_eq!(comment["result"]["comments"][0]["body"], "check this");

        let missing = reverse_rpc_response(
            &db,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "unknownTool"
            }),
            "session-1",
        );
        assert_eq!(missing["error"]["code"], -32601);
        Ok(())
    }

    #[test]
    fn reverse_rpc_tool_approval_respects_permission_mode() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let now = now_ms();
        let cwd = std::env::temp_dir().display().to_string();
        db.execute(
            "INSERT INTO repos (id, name, path, setup_script, run_script, run_script_mode, created_at, updated_at)
             VALUES ('repo-1', 'repo', ?1, '', '', 'concurrent', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'workspace', ?1, 'active', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO sessions (id, workspace_id, title, agent_type, permission_mode, created_at, updated_at)
             VALUES ('session-1', 'workspace-1', 'session', 'claude', 'auto', ?1, ?1)",
            params![now],
        )?;

        let approved = reverse_rpc_response(
            &db,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "toolApproval"
            }),
            "session-1",
        );
        assert_eq!(approved["result"]["approved"], true);
        assert_eq!(approved["result"]["permissionMode"], "auto");

        db.execute(
            "UPDATE sessions SET permission_mode = 'default' WHERE id = 'session-1'",
            [],
        )?;
        let denied = reverse_rpc_response(
            &db,
            &serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "toolApproval"
            }),
            "session-1",
        );
        assert_eq!(denied["result"]["approved"], false);
        Ok(())
    }

    #[test]
    fn estimates_context_usage_from_transcript() -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let now = now_ms();
        let cwd = std::env::temp_dir().display().to_string();
        db.execute(
            "INSERT INTO repos (id, name, path, setup_script, run_script, run_script_mode, created_at, updated_at)
             VALUES ('repo-1', 'repo', ?1, '', '', 'concurrent', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO workspaces (id, repo_id, name, path, state, created_at, updated_at)
             VALUES ('workspace-1', 'repo-1', 'workspace', ?1, 'active', ?2, ?2)",
            params![cwd, now],
        )?;
        db.execute(
            "INSERT INTO sessions (id, workspace_id, title, agent_type, model, permission_mode, created_at, updated_at)
             VALUES ('session-1', 'workspace-1', 'session', 'codex', 'gpt-5-codex', 'default', ?1, ?1)",
            params![now],
        )?;
        insert_message(&db, "session-1", "user", "abcdefghijklmnop", now)?;
        insert_message(&db, "session-1", "assistant", "abcdefgh", now)?;

        let usage = context_usage_for_db(&db, "session-1")?;
        assert_eq!(usage.used_tokens, 6);
        assert_eq!(usage.max_tokens, 272_000);
        assert!(usage.percent > 0.0);
        Ok(())
    }

    #[test]
    fn workspace_plan_reuses_suffix_and_reports_git_state() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let settings = load_settings(&db)?;
        let temp_root = std::env::temp_dir().join(format!("loomen-plan-{}", Uuid::new_v4()));
        let repo = temp_root.join("repo");
        std::fs::create_dir_all(&repo)?;

        let plan = build_workspace_plan(
            repo.to_str().unwrap(),
            "repo",
            Some("main"),
            Some("main"),
            &settings,
            "Polish Launch Health",
            "",
            Some("feature/base"),
            Some("abc12345"),
        );

        assert_eq!(plan.workspace_name, "Polish Launch Health");
        assert_eq!(plan.branch_leaf, "polish-launch-health-abc12345");
        assert!(plan.branch_name.ends_with("polish-launch-health-abc12345"));
        assert_eq!(plan.base_branch, "feature/base");
        assert_eq!(plan.checkpoint_id, "workspace-abc12345");
        assert!(plan
            .worktree_path
            .ends_with("polish-launch-health-abc12345"));
        assert!(plan.can_create);

        let _ = std::fs::remove_dir_all(&temp_root);
        Ok(())
    }

    #[test]
    fn workspace_plan_blocks_existing_non_empty_path() -> Result<(), Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        init_db(&db)?;
        let settings = load_settings(&db)?;
        let temp_root = std::env::temp_dir().join(format!("loomen-plan-{}", Uuid::new_v4()));
        let repo = temp_root.join("repo");
        let occupied = temp_root.join("occupied");
        std::fs::create_dir_all(&repo)?;
        std::fs::create_dir_all(&occupied)?;
        std::fs::write(occupied.join("README.md"), "already here\n")?;

        let plan = build_workspace_plan(
            repo.to_str().unwrap(),
            "repo",
            Some("main"),
            Some("main"),
            &settings,
            "",
            occupied.to_str().unwrap(),
            None,
            Some("abc12345"),
        );

        assert_eq!(plan.workspace_name, "workspace");
        assert!(plan.path_exists);
        assert!(!plan.path_is_empty);
        assert!(!plan.can_create);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("not empty")));

        let _ = std::fs::remove_dir_all(&temp_root);
        Ok(())
    }
}
