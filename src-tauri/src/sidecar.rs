use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use uuid::Uuid;

pub(crate) struct SidecarProcess {
    child: Child,
    socket_path: PathBuf,
}

impl Drop for SidecarProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl SidecarProcess {
    fn is_socket_ready(&self) -> bool {
        self.socket_path.exists()
    }
}

#[derive(Debug)]
pub(crate) struct SidecarHealth {
    pub(crate) status: &'static str,
    pub(crate) detail: String,
    pub(crate) path: Option<String>,
    pub(crate) remediation: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SidecarRuntimeSettings {
    pub(crate) provider_env: String,
    pub(crate) codex_provider_mode: String,
    pub(crate) default_codex_effort: String,
    pub(crate) codex_personality: String,
    pub(crate) claude_executable_path: String,
    pub(crate) codex_executable_path: String,
    pub(crate) claude_tool_approvals: bool,
}

pub(crate) struct SidecarQuery<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) prompt: &'a str,
    pub(crate) agent_type: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) model: Option<&'a str>,
    pub(crate) permission_mode: &'a str,
}

pub(crate) fn sidecar_health(sidecar: &mut Option<SidecarProcess>) -> SidecarHealth {
    match sidecar.as_mut() {
        None => SidecarHealth {
            status: "ok",
            detail: "Sidecar is idle and will start when Beam or workspace init needs it."
                .to_string(),
            path: None,
            remediation: None,
        },
        Some(process) => match process.child.try_wait() {
            Ok(None) if process.is_socket_ready() => SidecarHealth {
                status: "ok",
                detail: "Sidecar process is running and the socket exists.".to_string(),
                path: Some(process.socket_path.display().to_string()),
                remediation: None,
            },
            Ok(None) => SidecarHealth {
                status: "error",
                detail: "Sidecar process is running but its socket is missing.".to_string(),
                path: Some(process.socket_path.display().to_string()),
                remediation: Some(
                    "Restart the sidecar by starting a new Beam session.".to_string(),
                ),
            },
            Ok(Some(status)) => SidecarHealth {
                status: "error",
                detail: format!("Sidecar exited with status {status}."),
                path: Some(process.socket_path.display().to_string()),
                remediation: Some(
                    "Check Bun and sidecar/index.ts, then start a new Beam session.".to_string(),
                ),
            },
            Err(err) => SidecarHealth {
                status: "error",
                detail: format!("Could not inspect sidecar process: {err}"),
                path: Some(process.socket_path.display().to_string()),
                remediation: Some("Restart Loomen or start a new Beam session.".to_string()),
            },
        },
    }
}

pub(crate) fn sidecar_socket_path(
    rebuild_root: &Path,
    sidecar: &mut Option<SidecarProcess>,
) -> Result<PathBuf, String> {
    Ok(ensure_sidecar(rebuild_root, sidecar)?.socket_path.clone())
}

pub(crate) fn sidecar_rpc(
    socket_path: &Path,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let mut stream = connect_socket(socket_path)?;
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

pub(crate) fn cancel_query(socket_path: &Path, session_id: &str) -> Result<(), String> {
    let mut stream = connect_socket(socket_path)?;
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

pub(crate) fn send_query_to_sidecar<F, G, H>(
    socket_path: &Path,
    runtime_settings: &SidecarRuntimeSettings,
    query: SidecarQuery<'_>,
    mut handle_reverse_rpc: H,
    mut on_message: F,
    mut on_session_event: G,
) -> Result<String, String>
where
    F: FnMut(String),
    G: FnMut(Value),
    H: FnMut(&Value) -> Result<Value, String>,
{
    let mut stream = connect_socket(socket_path)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60 * 15)))
        .map_err(|err| err.to_string())?;

    let turn_id = Uuid::new_v4().to_string();
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": turn_id,
        "method": "query",
        "params": {
            "id": query.session_id,
            "prompt": query.prompt,
            "agentType": query.agent_type,
            "options": {
                "cwd": query.cwd,
                "model": query.model.unwrap_or(if query.agent_type == "codex" { "gpt-5-codex" } else { "opus" }),
                "permissionMode": query.permission_mode,
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
            let response = handle_reverse_rpc(&value)?;
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

fn connect_socket(socket_path: &Path) -> Result<UnixStream, String> {
    UnixStream::connect(socket_path).map_err(|err| {
        format!(
            "failed to connect sidecar socket {}: {}",
            socket_path.display(),
            err
        )
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_idle_is_a_healthy_lazy_start_state() {
        let mut sidecar = None;
        let check = sidecar_health(&mut sidecar);
        assert_eq!(check.status, "ok");
        assert!(check.detail.contains("idle"));
        assert!(check.remediation.is_none());
    }

    #[test]
    fn extracts_text_blocks_from_sidecar_message() {
        let value = serde_json::json!({
            "method": "message",
            "params": {
                "message": {
                    "content": [
                        { "type": "text", "text": "first" },
                        { "type": "tool_use", "name": "edit" },
                        { "type": "text", "text": "second" }
                    ]
                }
            }
        });
        assert_eq!(
            extract_sidecar_text(&value).as_deref(),
            Some("first\nsecond")
        );
    }

    #[test]
    fn extracts_query_error_text_from_known_shapes() {
        assert_eq!(
            extract_query_error_text(&serde_json::json!({ "params": { "error": "boom" } }))
                .as_deref(),
            Some("boom")
        );
        assert_eq!(
            extract_query_error_text(&serde_json::json!({ "params": { "message": "nope" } }))
                .as_deref(),
            Some("nope")
        );
        assert_eq!(
            extract_query_error_text(&serde_json::json!({ "params": { "text": "sad" } }))
                .as_deref(),
            Some("sad")
        );
    }
}
