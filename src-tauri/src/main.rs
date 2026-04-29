use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

struct AppState {
    db: Mutex<Connection>,
    db_path: PathBuf,
    rebuild_root: PathBuf,
    sidecar: Mutex<Option<SidecarProcess>>,
    ptys: Mutex<HashMap<String, PtySession>>,
    spotlighters: Mutex<HashMap<String, SpotlighterProcess>>,
    approvals: Mutex<HashMap<String, mpsc::Sender<ToolApprovalDecision>>>,
}

struct SidecarProcess {
    child: Child,
    socket_path: PathBuf,
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct PtySession {
    child: Child,
    master: File,
    output: Arc<Mutex<String>>,
    workspace_id: String,
    cwd: String,
    started_at: i64,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Repo {
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
struct Workspace {
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
struct Session {
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
struct Message {
    id: String,
    session_id: String,
    role: String,
    content: String,
    created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSnapshot {
    db_path: String,
    repos: Vec<Repo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    send_messages_with: String,
    desktop_notifications: bool,
    sound_effects: bool,
    auto_convert_long_text: bool,
    strip_absolute_right: bool,
    always_show_context_usage: bool,
    expand_tool_calls: bool,
    default_claude_model: String,
    default_codex_model: String,
    default_codex_effort: String,
    codex_personality: String,
    review_model: String,
    review_codex_effort: String,
    default_to_plan_mode: bool,
    default_to_fast_mode: bool,
    claude_chrome: bool,
    provider_env: String,
    codex_provider_mode: String,
    theme: String,
    colored_sidebar_diffs: bool,
    mono_font: String,
    markdown_style: String,
    terminal_font: String,
    terminal_font_size: i64,
    branch_prefix_type: String,
    branch_prefix_custom: String,
    delete_branch_on_archive: bool,
    archive_on_merge: bool,
    loomen_root_directory: String,
    claude_executable_path: String,
    codex_executable_path: String,
    big_terminal_mode: bool,
    dashboard: bool,
    voice_mode: bool,
    automerge: bool,
    spotlight_testing: bool,
    sidebar_resource_usage: bool,
    match_workspace_directory_with_branch_name: bool,
    experimental_terminal_runtime: bool,
    react_profiler: bool,
    enterprise_data_privacy: bool,
    claude_tool_approvals: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffOutput {
    workspace_id: String,
    checkpoint_id: Option<String>,
    diff: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalRun {
    id: String,
    workspace_id: String,
    command: String,
    cwd: String,
    output: String,
    exit_code: Option<i32>,
    started_at: i64,
    ended_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PtyTerminalInfo {
    id: String,
    workspace_id: String,
    cwd: String,
    output: String,
    is_running: bool,
    started_at: i64,
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
struct DiffFile {
    path: String,
    status: String,
    additions: usize,
    deletions: usize,
    patch: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffComment {
    id: String,
    workspace_id: String,
    file_path: String,
    line_number: i64,
    body: String,
    is_resolved: bool,
    created_at: i64,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestInfo {
    workspace_id: String,
    number: Option<i64>,
    title: Option<String>,
    url: Option<String>,
    state: Option<String>,
    is_draft: bool,
    head_ref_name: Option<String>,
    base_ref_name: Option<String>,
    checks: Vec<CheckInfo>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckInfo {
    name: String,
    kind: Option<String>,
    workflow_name: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    details_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
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
fn create_workspace(
    repo_id: String,
    name: String,
    path: String,
    base_branch: Option<String>,
    state: State<AppState>,
) -> Result<AppSnapshot, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    let settings = load_settings(&db).map_err(|err| err.to_string())?;
    let (repo_path, repo_name, current_branch, default_branch) = db
        .query_row(
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
        .ok_or_else(|| "repo not found".to_string())?;

    let workspace_name = if name.trim().is_empty() {
        "workspace"
    } else {
        name.trim()
    };
    let suffix = Uuid::new_v4().to_string()[..8].to_string();
    let slug = slugify(workspace_name);
    let branch_leaf = format!("{}-{}", slug, suffix);
    let branch_name = workspace_branch_name(&repo_path, &settings, &branch_leaf);
    let requested_base = base_branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let base_branch = requested_base
        .or(current_branch.as_deref())
        .or(default_branch.as_deref())
        .unwrap_or("HEAD")
        .to_string();
    let worktree_path = if path.trim().is_empty() {
        default_worktree_path(&settings, &repo_name, &branch_leaf)
    } else {
        expand_tilde(path.trim())
    };

    create_git_worktree(&repo_path, &worktree_path, &branch_name, &base_branch)?;
    let checkpoint_id = save_checkpoint(&worktree_path, &format!("workspace-{}", suffix)).ok();
    ensure_workspace(
        &db,
        &repo_id,
        workspace_name,
        &worktree_path,
        "active",
        Some(&branch_name),
        Some(&base_branch),
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
fn sidecar_status(state: State<AppState>) -> Result<String, String> {
    let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
    let process = ensure_sidecar(&state.rebuild_root, &mut sidecar)?;
    Ok(process.socket_path.display().to_string())
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
        ensure_sidecar(&state.rebuild_root, &mut sidecar)?
            .socket_path
            .clone()
    };
    let mut stream = UnixStream::connect(&socket_path).map_err(|err| {
        format!(
            "failed to connect sidecar socket {}: {}",
            socket_path.display(),
            err
        )
    })?;
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "cancel",
        "params": { "id": session_id }
    });
    writeln!(stream, "{}", payload).map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())?;
    Ok(())
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
    let run = run_shell_command(&workspace_id, &cwd, &script)?;
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
    let run = run_shell_command(&workspace_id, &cwd, &script)?;
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
    let run = run_shell_command(&workspace_id, &cwd, &command)?;
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
    let output = session.output.clone();
    let reader = session
        .master
        .try_clone()
        .map_err(|err| format!("failed to clone PTY master: {err}"))?;
    std::thread::spawn(move || read_pty_output(reader, output));

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
        if session.workspace_id == workspace_id {
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
    session
        .master
        .write_all(input.as_bytes())
        .map_err(|err| err.to_string())?;
    session.master.flush().map_err(|err| err.to_string())?;
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
    let _ = session.child.kill();
    let _ = session.child.wait();
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
        let _ = session.child.kill();
        let _ = session.child.wait();
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

fn get_pull_request_info_for_cwd(workspace_id: &str, cwd: &str) -> Result<PullRequestInfo, String> {
    let output = Command::new(gh_path())
        .arg("pr")
        .arg("view")
        .arg("--json")
        .arg("number,title,url,state,isDraft,headRefName,baseRefName,statusCheckRollup")
        .current_dir(&cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        parse_pr_info_json(workspace_id, &String::from_utf8_lossy(&output.stdout))
    } else {
        Ok(PullRequestInfo {
            workspace_id: workspace_id.to_string(),
            number: None,
            title: None,
            url: None,
            state: None,
            is_draft: false,
            head_ref_name: None,
            base_ref_name: None,
            checks: Vec::new(),
            error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        })
    }
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

    let title = title.trim();
    if title.is_empty() {
        return Err("PR title is required".to_string());
    }
    let mut command = Command::new(gh_path());
    command
        .arg("pr")
        .arg("create")
        .arg("--title")
        .arg(title)
        .arg("--body")
        .arg(if body.trim().is_empty() {
            "Created from Loomen"
        } else {
            body.trim()
        })
        .current_dir(&cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    if let Some(base) = base_branch.as_deref().filter(|base| !base.is_empty()) {
        command.arg("--base").arg(base);
    }
    if let Some(head) = branch_name.as_deref().filter(|head| !head.is_empty()) {
        command.arg("--head").arg(head);
    }
    if draft {
        command.arg("--draft");
    }
    let output = command.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Ok(PullRequestInfo {
            workspace_id,
            number: None,
            title: None,
            url: None,
            state: None,
            is_draft: false,
            head_ref_name: branch_name,
            base_ref_name: base_branch,
            checks: Vec::new(),
            error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        });
    }

    let view = Command::new(gh_path())
        .arg("pr")
        .arg("view")
        .arg("--json")
        .arg("number,title,url,state,isDraft,headRefName,baseRefName,statusCheckRollup")
        .current_dir(&cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .output()
        .map_err(|err| err.to_string())?;
    if view.status.success() {
        parse_pr_info_json(&workspace_id, &String::from_utf8_lossy(&view.stdout))
    } else {
        Ok(PullRequestInfo {
            workspace_id,
            number: None,
            title: Some(title.to_string()),
            url: String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .find(|part| part.starts_with("http"))
                .map(str::to_string),
            state: Some("OPEN".to_string()),
            is_draft: draft,
            head_ref_name: branch_name,
            base_ref_name: base_branch,
            checks: Vec::new(),
            error: None,
        })
    }
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

    let title = title.trim();
    if title.is_empty() {
        return Err("PR title is required".to_string());
    }
    let output = Command::new(gh_path())
        .arg("pr")
        .arg("edit")
        .arg("--title")
        .arg(title)
        .arg("--body")
        .arg(if body.trim().is_empty() {
            "Updated from Loomen"
        } else {
            body.trim()
        })
        .current_dir(&cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Ok(PullRequestInfo {
            workspace_id,
            number: None,
            title: None,
            url: None,
            state: None,
            is_draft: false,
            head_ref_name: None,
            base_ref_name: None,
            checks: Vec::new(),
            error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        });
    }
    get_pull_request_info_for_cwd(&workspace_id, &cwd)
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
    let list_output = Command::new(gh_path())
        .arg("run")
        .arg("list")
        .arg("--branch")
        .arg(&branch)
        .arg("--limit")
        .arg("1")
        .arg("--json")
        .arg("databaseId,status,conclusion,name")
        .current_dir(&cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .output()
        .map_err(|err| err.to_string())?;
    if !list_output.status.success() {
        return Err(String::from_utf8_lossy(&list_output.stderr)
            .trim()
            .to_string());
    }
    let runs: Value = serde_json::from_slice(&list_output.stdout).map_err(|err| err.to_string())?;
    let run = runs
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| format!("no GitHub Actions runs found for branch {branch}"))?;
    let run_id = run
        .get("databaseId")
        .and_then(Value::as_i64)
        .ok_or_else(|| "latest run did not include databaseId".to_string())?;
    let rerun_output = Command::new(gh_path())
        .arg("run")
        .arg("rerun")
        .arg(run_id.to_string())
        .arg("--failed")
        .current_dir(&cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .output()
        .map_err(|err| err.to_string())?;
    if rerun_output.status.success() {
        let run_name = run
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("workflow");
        Ok(format!(
            "Rerun requested for failed jobs in {run_name} ({run_id})."
        ))
    } else {
        Err(String::from_utf8_lossy(&rerun_output.stderr)
            .trim()
            .to_string())
    }
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
    let id = Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO diff_comments (id, workspace_id, file_path, line_number, body, is_resolved, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
        params![id, workspace_id, file_path, line_number, body, now_ms()],
    )
    .map_err(|err| err.to_string())?;
    list_diff_comments_for_db(&db, &workspace_id)
}

#[tauri::command]
fn resolve_diff_comment(
    comment_id: String,
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<DiffComment>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    db.execute(
        "UPDATE diff_comments SET is_resolved = 1 WHERE id = ?1 AND workspace_id = ?2",
        params![comment_id, workspace_id],
    )
    .map_err(|err| err.to_string())?;
    list_diff_comments_for_db(&db, &workspace_id)
}

#[tauri::command]
fn list_diff_comments(
    workspace_id: String,
    state: State<AppState>,
) -> Result<Vec<DiffComment>, String> {
    let db = state.db.lock().map_err(|err| err.to_string())?;
    list_diff_comments_for_db(&db, &workspace_id)
}

fn load_snapshot(db: &Connection, db_path: &Path) -> Result<AppSnapshot, String> {
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

fn init_db(db: &Connection) -> anyhow::Result<()> {
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
            command TEXT NOT NULL,
            cwd TEXT NOT NULL,
            output TEXT NOT NULL,
            exit_code INTEGER,
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
    Ok(())
}

fn load_settings(db: &Connection) -> anyhow::Result<AppSettings> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    Ok(AppSettings {
        send_messages_with: setting_string(db, "send_messages_with", "Enter")?,
        desktop_notifications: setting_bool(db, "desktop_notifications", false)?,
        sound_effects: setting_bool(db, "sound_effects", false)?,
        auto_convert_long_text: setting_bool(db, "auto_convert_long_text", true)?,
        strip_absolute_right: setting_bool(db, "strip_absolute_right", false)?,
        always_show_context_usage: setting_bool(db, "always_show_context_usage", false)?,
        expand_tool_calls: setting_bool(db, "expand_tool_calls", false)?,
        default_claude_model: setting_string(db, "default_claude_model", "opus")?,
        default_codex_model: setting_string(db, "default_codex_model", "gpt-5-codex")?,
        default_codex_effort: setting_string(db, "default_codex_effort", "high")?,
        codex_personality: setting_string(db, "codex_personality", "Default")?,
        review_model: setting_string(db, "review_model", "opus")?,
        review_codex_effort: setting_string(db, "review_codex_effort", "high")?,
        default_to_plan_mode: setting_bool(db, "default_to_plan_mode", true)?,
        default_to_fast_mode: setting_bool(db, "default_to_fast_mode", false)?,
        claude_chrome: setting_bool(db, "claude_chrome", false)?,
        provider_env: setting_string(db, "provider_env", "")?,
        codex_provider_mode: setting_string(db, "codex_provider_mode", "cli")?,
        theme: setting_string(db, "theme", "Dark")?,
        colored_sidebar_diffs: setting_bool(db, "colored_sidebar_diffs", false)?,
        mono_font: setting_string(db, "mono_font", "Geist Mono")?,
        markdown_style: setting_string(db, "markdown_style", "Default")?,
        terminal_font: setting_string(db, "terminal_font", "")?,
        terminal_font_size: setting_i64(db, "terminal_font_size", 12)?,
        branch_prefix_type: setting_string(db, "branch_prefix_type", "github_username")?,
        branch_prefix_custom: setting_string(db, "branch_prefix_custom", "")?,
        delete_branch_on_archive: setting_bool(db, "delete_branch_on_archive", false)?,
        archive_on_merge: setting_bool(db, "archive_on_merge", false)?,
        loomen_root_directory: setting_string(
            db,
            "loomen_root_directory",
            &format!("{home}/loomen"),
        )?,
        claude_executable_path: setting_string(db, "claude_executable_path", "")?,
        codex_executable_path: setting_string(db, "codex_executable_path", "")?,
        big_terminal_mode: setting_bool(db, "big_terminal_mode", false)?,
        dashboard: setting_bool(db, "dashboard", false)?,
        voice_mode: setting_bool(db, "voice_mode", false)?,
        automerge: setting_bool(db, "automerge", false)?,
        spotlight_testing: setting_bool(db, "spotlight_testing", false)?,
        sidebar_resource_usage: setting_bool(db, "sidebar_resource_usage", false)?,
        match_workspace_directory_with_branch_name: setting_bool(
            db,
            "match_workspace_directory_with_branch_name",
            false,
        )?,
        experimental_terminal_runtime: setting_bool(db, "experimental_terminal_runtime", false)?,
        react_profiler: setting_bool(db, "react_profiler", false)?,
        enterprise_data_privacy: setting_bool(db, "enterprise_data_privacy", false)?,
        claude_tool_approvals: setting_bool(db, "claude_tool_approvals", false)?,
    })
}

fn save_settings(db: &Connection, settings: &AppSettings) -> anyhow::Result<()> {
    put_setting(db, "send_messages_with", &settings.send_messages_with)?;
    put_setting(
        db,
        "desktop_notifications",
        bool_value(settings.desktop_notifications),
    )?;
    put_setting(db, "sound_effects", bool_value(settings.sound_effects))?;
    put_setting(
        db,
        "auto_convert_long_text",
        bool_value(settings.auto_convert_long_text),
    )?;
    put_setting(
        db,
        "strip_absolute_right",
        bool_value(settings.strip_absolute_right),
    )?;
    put_setting(
        db,
        "always_show_context_usage",
        bool_value(settings.always_show_context_usage),
    )?;
    put_setting(
        db,
        "expand_tool_calls",
        bool_value(settings.expand_tool_calls),
    )?;
    put_setting(db, "default_claude_model", &settings.default_claude_model)?;
    put_setting(db, "default_codex_model", &settings.default_codex_model)?;
    put_setting(db, "default_codex_effort", &settings.default_codex_effort)?;
    put_setting(db, "codex_personality", &settings.codex_personality)?;
    put_setting(db, "review_model", &settings.review_model)?;
    put_setting(db, "review_codex_effort", &settings.review_codex_effort)?;
    put_setting(
        db,
        "default_to_plan_mode",
        bool_value(settings.default_to_plan_mode),
    )?;
    put_setting(
        db,
        "default_to_fast_mode",
        bool_value(settings.default_to_fast_mode),
    )?;
    put_setting(db, "claude_chrome", bool_value(settings.claude_chrome))?;
    put_setting(db, "provider_env", &settings.provider_env)?;
    put_setting(db, "codex_provider_mode", &settings.codex_provider_mode)?;
    put_setting(db, "theme", &settings.theme)?;
    put_setting(
        db,
        "colored_sidebar_diffs",
        bool_value(settings.colored_sidebar_diffs),
    )?;
    put_setting(db, "mono_font", &settings.mono_font)?;
    put_setting(db, "markdown_style", &settings.markdown_style)?;
    put_setting(db, "terminal_font", &settings.terminal_font)?;
    put_setting(
        db,
        "terminal_font_size",
        &settings.terminal_font_size.to_string(),
    )?;
    put_setting(db, "branch_prefix_type", &settings.branch_prefix_type)?;
    put_setting(db, "branch_prefix_custom", &settings.branch_prefix_custom)?;
    put_setting(
        db,
        "delete_branch_on_archive",
        bool_value(settings.delete_branch_on_archive),
    )?;
    put_setting(
        db,
        "archive_on_merge",
        bool_value(settings.archive_on_merge),
    )?;
    put_setting(db, "loomen_root_directory", &settings.loomen_root_directory)?;
    put_setting(
        db,
        "claude_executable_path",
        &settings.claude_executable_path,
    )?;
    put_setting(db, "codex_executable_path", &settings.codex_executable_path)?;
    put_setting(
        db,
        "big_terminal_mode",
        bool_value(settings.big_terminal_mode),
    )?;
    put_setting(db, "dashboard", bool_value(settings.dashboard))?;
    put_setting(db, "voice_mode", bool_value(settings.voice_mode))?;
    put_setting(db, "automerge", bool_value(settings.automerge))?;
    put_setting(
        db,
        "spotlight_testing",
        bool_value(settings.spotlight_testing),
    )?;
    put_setting(
        db,
        "sidebar_resource_usage",
        bool_value(settings.sidebar_resource_usage),
    )?;
    put_setting(
        db,
        "match_workspace_directory_with_branch_name",
        bool_value(settings.match_workspace_directory_with_branch_name),
    )?;
    put_setting(
        db,
        "experimental_terminal_runtime",
        bool_value(settings.experimental_terminal_runtime),
    )?;
    put_setting(db, "react_profiler", bool_value(settings.react_profiler))?;
    put_setting(
        db,
        "enterprise_data_privacy",
        bool_value(settings.enterprise_data_privacy),
    )?;
    put_setting(
        db,
        "claude_tool_approvals",
        bool_value(settings.claude_tool_approvals),
    )?;
    Ok(())
}

fn setting_string(db: &Connection, key: &str, default: &str) -> anyhow::Result<String> {
    Ok(db
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_else(|| default.to_string()))
}

fn setting_bool(db: &Connection, key: &str, default: bool) -> anyhow::Result<bool> {
    let value = setting_string(db, key, bool_value(default))?;
    Ok(matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    ))
}

fn setting_i64(db: &Connection, key: &str, default: i64) -> anyhow::Result<i64> {
    let value = setting_string(db, key, &default.to_string())?;
    Ok(value.trim().parse::<i64>().unwrap_or(default))
}

fn put_setting(db: &Connection, key: &str, value: &str) -> anyhow::Result<()> {
    db.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn bool_value(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn default_model_for_agent(settings: &AppSettings, agent_type: &str) -> String {
    if agent_type == "codex" {
        non_empty(&settings.default_codex_model, "gpt-5-codex")
    } else {
        non_empty(&settings.default_claude_model, "opus")
    }
}

fn default_permission_mode(settings: &AppSettings) -> &'static str {
    if settings.default_to_plan_mode {
        "plan"
    } else if settings.default_to_fast_mode {
        "dontAsk"
    } else {
        "default"
    }
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

fn run_shell_command(workspace_id: &str, cwd: &str, command: &str) -> Result<TerminalRun, String> {
    let started_at = now_ms();
    let output = Command::new("/bin/zsh")
        .arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .output()
        .map_err(|err| err.to_string())?;
    let ended_at = now_ms();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = if stderr.trim().is_empty() {
        stdout.to_string()
    } else if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        format!("{stdout}\n{stderr}")
    };
    Ok(TerminalRun {
        id: Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        command: command.to_string(),
        cwd: cwd.to_string(),
        output: combined,
        exit_code: output.status.code(),
        started_at,
        ended_at,
    })
}

fn store_terminal_run(db: &Connection, run: &TerminalRun) -> Result<(), String> {
    db.execute(
        "INSERT INTO terminal_sessions (id, workspace_id, command, cwd, output, exit_code, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            run.id,
            run.workspace_id,
            run.command,
            run.cwd,
            run.output,
            run.exit_code,
            run.started_at,
            run.ended_at
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

fn upsert_pty_terminal_snapshot(db: &Connection, info: &PtyTerminalInfo) -> Result<(), String> {
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

fn load_pty_terminal_snapshots(
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

fn load_pty_terminal_snapshot(
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

fn delete_pty_terminal_snapshot(db: &Connection, terminal_id: &str) -> Result<(), String> {
    db.execute(
        "DELETE FROM pty_terminal_tabs WHERE id = ?1",
        params![terminal_id],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
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

fn resolve_git_root(path: &str) -> Result<String, String> {
    let expanded = expand_tilde(path);
    git_output(&expanded, &["rev-parse", "--show-toplevel"])
}

fn detect_default_branch(repo_path: &str) -> Option<String> {
    git_output(
        repo_path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .and_then(|branch| branch.strip_prefix("origin/").map(str::to_string))
    .or_else(|| {
        for candidate in ["main", "master", "develop"] {
            if git_output(repo_path, &["rev-parse", "--verify", candidate]).is_ok() {
                return Some(candidate.to_string());
            }
        }
        None
    })
}

fn list_git_branches(
    repo_path: &str,
    current_branch: Option<&str>,
    default_branch: Option<&str>,
) -> Vec<String> {
    let mut branches = Vec::new();
    for branch in [current_branch, default_branch, Some("HEAD")]
        .into_iter()
        .flatten()
    {
        push_unique_branch(&mut branches, branch);
    }
    if let Ok(output) = git_output(
        repo_path,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    ) {
        for line in output.lines() {
            let branch = line.trim();
            if branch.is_empty() || branch == "origin/HEAD" || branch.ends_with("/HEAD") {
                continue;
            }
            push_unique_branch(&mut branches, branch);
        }
    }
    branches.truncate(80);
    branches
}

fn push_unique_branch(branches: &mut Vec<String>, branch: &str) {
    let branch = branch.trim();
    if branch.is_empty() || branches.iter().any(|item| item == branch) {
        return;
    }
    branches.push(branch.to_string());
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

fn create_git_worktree(
    repo_path: &str,
    worktree_path: &str,
    branch_name: &str,
    base_branch: &str,
) -> Result<(), String> {
    let path = Path::new(worktree_path);
    if path.exists()
        && path
            .read_dir()
            .map_err(|err| err.to_string())?
            .next()
            .is_some()
    {
        return Err(format!("worktree path is not empty: {worktree_path}"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    git_status(
        repo_path,
        &[
            "worktree",
            "add",
            "-b",
            branch_name,
            worktree_path,
            base_branch,
        ],
    )
}

fn save_checkpoint(worktree_path: &str, id: &str) -> Result<String, String> {
    let id = sanitize_checkpoint_id(id)?;
    let ref_name = checkpoint_ref(&id)?;
    let temp_index =
        std::env::temp_dir().join(format!("loomen-checkpoint-{}.index", Uuid::new_v4()));
    let temp_index_value = temp_index.to_string_lossy().to_string();
    let envs = [("GIT_INDEX_FILE", temp_index_value.as_str())];

    let result = (|| -> Result<String, String> {
        git_output_with_env(worktree_path, &["read-tree", "HEAD"], &envs)?;
        git_output_with_env(worktree_path, &["add", "-A", "--", "."], &envs)?;
        let tree = git_output_with_env(worktree_path, &["write-tree"], &envs)?;
        let parent = git_output(worktree_path, &["rev-parse", "HEAD"])?;
        let message = format!("Loomen checkpoint {id}");
        let commit = git_output_with_env(
            worktree_path,
            &[
                "commit-tree",
                tree.trim(),
                "-p",
                parent.trim(),
                "-m",
                &message,
            ],
            &envs,
        )?;
        git_output(worktree_path, &["update-ref", &ref_name, commit.trim()])?;
        Ok(id)
    })();

    let _ = std::fs::remove_file(temp_index);
    result
}

fn checkpoint_diff(worktree_path: &str, id: &str, mode: &str) -> Result<String, String> {
    let ref_name = checkpoint_ref(&sanitize_checkpoint_id(id)?)?;
    if mode == "--stat" {
        git_output(worktree_path, &["diff", &ref_name, "--stat"])
    } else {
        git_output(worktree_path, &["diff", &ref_name])
    }
}

fn spawn_pty_shell(workspace_id: &str, cwd: &str, started_at: i64) -> Result<PtySession, String> {
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

fn read_pty_output(mut reader: File, output: Arc<Mutex<String>>) {
    let mut buffer = [0u8; 4096];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let chunk = String::from_utf8_lossy(&buffer[..n]);
                if let Ok(mut out) = output.lock() {
                    out.push_str(&chunk);
                    let keep_from = out.len().saturating_sub(200_000);
                    if keep_from > 0 {
                        let trimmed = out[keep_from..].to_string();
                        *out = trimmed;
                    }
                }
            }
            Err(_) => break,
        }
    }
}

fn terminal_info(id: &str, session: &mut PtySession) -> Result<PtyTerminalInfo, String> {
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

fn parse_diff_files(raw: &str) -> Vec<DiffFile> {
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

fn list_diff_comments_for_db(
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

fn git_output(repo_path: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {:?} failed", args)
        } else {
            stderr
        })
    }
}

fn git_output_with_env(
    repo_path: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .envs(envs.iter().copied())
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {:?} failed", args)
        } else {
            stderr
        })
    }
}

fn sanitize_checkpoint_id(id: &str) -> Result<String, String> {
    let id = id.trim().trim_matches('/').to_string();
    let valid = !id.is_empty()
        && !id.contains("..")
        && !id.starts_with('-')
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/'));
    if valid {
        Ok(id)
    } else {
        Err("invalid checkpoint id".to_string())
    }
}

fn checkpoint_ref(id: &str) -> Result<String, String> {
    Ok(format!(
        "refs/loomen-checkpoints/{}",
        sanitize_checkpoint_id(id)?
    ))
}

fn git_status(repo_path: &str, args: &[&str]) -> Result<(), String> {
    git_output(repo_path, args).map(|_| ())
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

fn spotlighter_path() -> PathBuf {
    rebuild_root().join("script").join("spotlighter.sh")
}

fn gh_path() -> PathBuf {
    PathBuf::from("gh")
}

fn check_string_field(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        item.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn parse_pr_info_json(workspace_id: &str, raw: &str) -> Result<PullRequestInfo, String> {
    let value: Value = serde_json::from_str(raw).map_err(|err| err.to_string())?;
    let checks = value
        .get("statusCheckRollup")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| CheckInfo {
                    name: check_string_field(item, &["name", "context", "workflowName"])
                        .unwrap_or_else(|| "check".to_string()),
                    kind: check_string_field(item, &["__typename", "kind", "type"]),
                    workflow_name: check_string_field(item, &["workflowName", "workflow_name"]),
                    status: check_string_field(item, &["status", "state"]),
                    conclusion: check_string_field(item, &["conclusion"]),
                    details_url: check_string_field(
                        item,
                        &[
                            "detailsUrl",
                            "details_url",
                            "targetUrl",
                            "target_url",
                            "url",
                        ],
                    ),
                    started_at: check_string_field(item, &["startedAt", "started_at"]),
                    completed_at: check_string_field(item, &["completedAt", "completed_at"]),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(PullRequestInfo {
        workspace_id: workspace_id.to_string(),
        number: value.get("number").and_then(Value::as_i64),
        title: value
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        url: value.get("url").and_then(Value::as_str).map(str::to_string),
        state: value
            .get("state")
            .and_then(Value::as_str)
            .map(str::to_string),
        is_draft: value
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        head_ref_name: value
            .get("headRefName")
            .and_then(Value::as_str)
            .map(str::to_string),
        base_ref_name: value
            .get("baseRefName")
            .and_then(Value::as_str)
            .map(str::to_string),
        checks,
        error: None,
    })
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
        ensure_sidecar(&state.rebuild_root, &mut sidecar)?
            .socket_path
            .clone()
    };
    let mut stream = UnixStream::connect(&socket_path).map_err(|err| {
        format!(
            "failed to connect sidecar socket {}: {}",
            socket_path.display(),
            err
        )
    })?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|err| err.to_string())?;
    let request_id = Uuid::new_v4().to_string();
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": method,
        "params": params
    });
    writeln!(stream, "{}", payload).map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if bytes == 0 {
            return Err("sidecar closed the socket before sending a response".to_string());
        }
        let value: Value = serde_json::from_str(line.trim()).map_err(|err| err.to_string())?;
        if value.get("id").and_then(Value::as_str) != Some(request_id.as_str()) {
            continue;
        }
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
        {
            return Err(message.to_string());
        }
        return Ok(value.get("result").cloned().unwrap_or(Value::Null));
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
    let runtime_settings = {
        let db = state.db.lock().map_err(|err| err.to_string())?;
        load_settings(&db).map_err(|err| err.to_string())?
    };
    let socket_path = {
        let mut sidecar = state.sidecar.lock().map_err(|err| err.to_string())?;
        ensure_sidecar(&state.rebuild_root, &mut sidecar)?
            .socket_path
            .clone()
    };

    let mut stream = UnixStream::connect(&socket_path).map_err(|err| {
        format!(
            "failed to connect sidecar socket {}: {}",
            socket_path.display(),
            err
        )
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60 * 15)))
        .map_err(|err| err.to_string())?;

    let turn_id = Uuid::new_v4().to_string();
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": turn_id,
        "method": "query",
        "params": {
            "id": session_id,
                "prompt": prompt,
                "agentType": agent_type,
                "options": {
                    "cwd": cwd,
                    "model": model.unwrap_or(if agent_type == "codex" { "gpt-5-codex" } else { "opus" }),
                    "permissionMode": permission_mode,
                    "providerEnv": runtime_settings.provider_env,
                    "codexProviderMode": runtime_settings.codex_provider_mode,
                    "codexEffort": runtime_settings.default_codex_effort,
                    "codexPersonality": runtime_settings.codex_personality,
                    "claudeExecutablePath": runtime_settings.claude_executable_path,
                    "codexExecutablePath": runtime_settings.codex_executable_path,
                    "claudeToolApprovals": runtime_settings.claude_tool_approvals,
                    "turnId": turn_id,
                    "shouldResetGenerator": false
                }
            }
    });
    writeln!(stream, "{}", payload).map_err(|err| err.to_string())?;
    stream.flush().map_err(|err| err.to_string())?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let mut streamed_messages = Vec::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if bytes == 0 {
            if streamed_messages.is_empty() {
                return Err("sidecar closed the socket before sending a response".to_string());
            }
            return Ok(streamed_messages.join("\n"));
        }
        let value: Value = serde_json::from_str(line.trim()).map_err(|err| err.to_string())?;
        if value.get("id").is_some() && value.get("method").and_then(Value::as_str).is_some() {
            let response = handle_reverse_rpc(state, app_handle, &value, session_id)?;
            writeln!(reader.get_mut(), "{}", response).map_err(|err| err.to_string())?;
            reader.get_mut().flush().map_err(|err| err.to_string())?;
            continue;
        }
        if value.get("id").and_then(Value::as_str) == Some(turn_id.as_str()) {
            if let Some(error) = value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
            {
                return Err(error.to_string());
            }
            let result_text = value
                .get("result")
                .and_then(|result| result.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("");
            return Ok(if streamed_messages.is_empty() {
                if result_text.is_empty() {
                    value.to_string()
                } else {
                    result_text.to_string()
                }
            } else {
                streamed_messages.join("\n")
            });
        }
        match value.get("method").and_then(Value::as_str) {
            Some("message") => {
                if let Some(text) = extract_sidecar_text(&value) {
                    on_message(text.clone());
                    streamed_messages.push(text);
                }
            }
            Some("queryError") => {
                streamed_messages.push(
                    extract_query_error_text(&value)
                        .unwrap_or_else(|| "sidecar query failed".to_string()),
                );
                continue;
            }
            Some("sessionEventNotification") => {
                if let Some(event) = value.get("params").cloned() {
                    on_session_event(event);
                }
                continue;
            }
            _ => continue,
        }
    }
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
    let file_path = params
        .get("filePath")
        .or_else(|| params.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
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
    Ok(serde_json::json!({
        "workspaceId": workspace_id,
        "comments": list_diff_comments_for_db(db, &workspace_id)?
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

fn ensure_sidecar<'a>(
    rebuild_root: &Path,
    sidecar: &'a mut Option<SidecarProcess>,
) -> Result<&'a SidecarProcess, String> {
    let needs_start = match sidecar.as_mut() {
        Some(process) => match process.child.try_wait() {
            Ok(Some(_)) => true,
            Ok(None) => !process.socket_path.exists(),
            Err(_) => true,
        },
        None => true,
    };

    if needs_start {
        *sidecar = Some(start_sidecar(rebuild_root)?);
    }

    sidecar
        .as_ref()
        .ok_or_else(|| "sidecar failed to start".to_string())
}

fn start_sidecar(rebuild_root: &Path) -> Result<SidecarProcess, String> {
    let mut child = Command::new("bun")
        .arg("sidecar/index.ts")
        .current_dir(rebuild_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to spawn bun sidecar: {err}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sidecar stdout was not piped".to_string())?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|err| err.to_string())?;
    let socket_path = line
        .trim()
        .strip_prefix("SOCKET_PATH=")
        .map(PathBuf::from)
        .ok_or_else(|| format!("sidecar did not report SOCKET_PATH, got: {}", line.trim()))?;

    Ok(SidecarProcess { child, socket_path })
}

fn extract_sidecar_text(value: &Value) -> Option<String> {
    let message = value.get("params")?.get("message")?;
    let content = message.get("content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_query_error_text(value: &Value) -> Option<String> {
    let params = value.get("params")?;
    params
        .get("error")
        .and_then(Value::as_str)
        .or_else(|| params.get("message").and_then(Value::as_str))
        .or_else(|| params.get("text").and_then(Value::as_str))
        .map(str::to_string)
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
            create_workspace,
            create_session,
            close_session,
            send_query,
            start_query,
            update_session_settings,
            get_context_usage,
            resolve_tool_approval,
            get_db_path,
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
            save_workspace_checkpoint,
            get_workspace_diff,
            run_terminal_command,
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

    #[test]
    fn creates_real_git_worktree_and_checkpoint_ref() -> Result<(), Box<dyn std::error::Error>> {
        let temp_root = std::env::temp_dir().join(format!("loomen-test-{}", Uuid::new_v4()));
        let repo = temp_root.join("repo");
        let worktree = temp_root.join("worktree");
        std::fs::create_dir_all(&repo)?;

        run_command("git", &["init", "-b", "main"], &repo)?;
        run_command("git", &["config", "user.name", "Loomen Test"], &repo)?;
        run_command(
            "git",
            &["config", "user.email", "loomen-test@example.invalid"],
            &repo,
        )?;
        std::fs::write(repo.join("README.md"), "hello\n")?;
        run_command("git", &["add", "README.md"], &repo)?;
        run_command("git", &["commit", "-m", "initial"], &repo)?;

        create_git_worktree(
            repo.to_str().unwrap(),
            worktree.to_str().unwrap(),
            "loomen/test-worktree",
            "main",
        )?;
        assert!(worktree.join("README.md").exists());

        let checkpoint = save_checkpoint(worktree.to_str().unwrap(), "test-checkpoint")?;
        assert_eq!(checkpoint, "test-checkpoint");
        let checkpoint_oid = git_output(
            repo.to_str().unwrap(),
            &["rev-parse", "refs/loomen-checkpoints/test-checkpoint"],
        )?;
        assert!(!checkpoint_oid.is_empty());

        let _ = run_command(
            "git",
            &["worktree", "remove", "--force", worktree.to_str().unwrap()],
            &repo,
        );
        let _ = std::fs::remove_dir_all(&temp_root);
        Ok(())
    }

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
    fn parses_rg_search_match() {
        let item = parse_rg_match("./src/main.rs:42:7:fn search_workspace()").unwrap();
        assert_eq!(item.path, "src/main.rs");
        assert_eq!(item.line, 42);
        assert_eq!(item.column, 7);
        assert_eq!(item.text, "fn search_workspace()");
    }

    #[test]
    fn starts_pty_shell_and_captures_scrollback() -> Result<(), Box<dyn std::error::Error>> {
        let cwd = std::env::temp_dir();
        let mut session = spawn_pty_shell("workspace-test", cwd.to_str().unwrap(), now_ms())?;
        let output = session.output.clone();
        let reader = session.master.try_clone()?;
        std::thread::spawn(move || read_pty_output(reader, output));

        session.master.write_all(b"echo PTY_OK\nexit\n")?;
        session.master.flush()?;
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
        init_db(&db)?;
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
        let setup_run = run_shell_command("workspace-1", &setup_cwd, &setup_script)?;
        assert_eq!(setup_run.exit_code, Some(0));
        assert!(setup_run.output.contains("SETUP_OK"));
        let setup_log = write_lifecycle_log("workspace-1", "setup", &setup_run.output)?;
        assert!(std::fs::read_to_string(setup_log)?.contains("SETUP_OK"));
        store_terminal_run(&db, &setup_run)?;

        let (_, run_script) = workspace_script(&db, "workspace-1", "run_script")?;
        let run = run_shell_command("workspace-1", &cwd, &run_script)?;
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
    fn parses_pull_request_info_and_checks() -> Result<(), Box<dyn std::error::Error>> {
        let raw = r#"{
          "number": 42,
          "title": "Add Loomen parity",
          "url": "https://github.com/example/repo/pull/42",
          "state": "OPEN",
          "isDraft": false,
          "headRefName": "loomen/helsinki",
          "baseRefName": "main",
          "statusCheckRollup": [
            {
              "name": "test",
              "__typename": "CheckRun",
              "workflowName": "CI",
              "status": "COMPLETED",
              "conclusion": "SUCCESS",
              "detailsUrl": "https://github.com/example/repo/actions/runs/1",
              "startedAt": "2026-04-30T10:00:00Z",
              "completedAt": "2026-04-30T10:02:30Z"
            },
            {
              "__typename": "StatusContext",
              "context": "deploy",
              "state": "PENDING",
              "targetUrl": "https://ci.example.invalid/deploy"
            },
            {
              "workflowName": "Lint",
              "status": "QUEUED"
            }
          ]
        }"#;
        let info = parse_pr_info_json("workspace-1", raw)?;
        assert_eq!(info.number, Some(42));
        assert_eq!(info.title.as_deref(), Some("Add Loomen parity"));
        assert_eq!(info.checks.len(), 3);
        assert_eq!(info.checks[0].name, "test");
        assert_eq!(info.checks[0].kind.as_deref(), Some("CheckRun"));
        assert_eq!(info.checks[0].workflow_name.as_deref(), Some("CI"));
        assert_eq!(info.checks[0].conclusion.as_deref(), Some("SUCCESS"));
        assert_eq!(
            info.checks[0].started_at.as_deref(),
            Some("2026-04-30T10:00:00Z")
        );
        assert_eq!(
            info.checks[0].completed_at.as_deref(),
            Some("2026-04-30T10:02:30Z")
        );
        assert_eq!(info.checks[1].name, "deploy");
        assert_eq!(info.checks[1].status.as_deref(), Some("PENDING"));
        assert_eq!(
            info.checks[1].details_url.as_deref(),
            Some("https://ci.example.invalid/deploy")
        );
        assert_eq!(info.checks[2].name, "Lint");
        assert_eq!(info.checks[2].status.as_deref(), Some("QUEUED"));
        Ok(())
    }

    fn run_command(cmd: &str, args: &[&str], cwd: &Path) -> Result<(), String> {
        let output = Command::new(cmd)
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}
