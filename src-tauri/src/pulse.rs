use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalRun {
    pub(crate) id: String,
    pub(crate) workspace_id: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) command: String,
    pub(crate) cwd: String,
    pub(crate) output: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) checkpoint_id: Option<String>,
    pub(crate) duration_ms: i64,
    pub(crate) started_at: i64,
    pub(crate) ended_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NamedPulse {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) command: String,
    pub(crate) detail: String,
    pub(crate) source: String,
}

pub(crate) fn run_shell_command(
    workspace_id: &str,
    cwd: &str,
    command: &str,
    kind: &str,
    label: &str,
    checkpoint_id: Option<&str>,
) -> Result<TerminalRun, String> {
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
        kind: normalized_pulse_kind(kind).to_string(),
        label: pulse_label(kind, label, command),
        command: command.to_string(),
        cwd: cwd.to_string(),
        output: combined,
        exit_code: output.status.code(),
        checkpoint_id: checkpoint_id
            .map(str::to_string)
            .filter(|id| !id.is_empty()),
        duration_ms: ended_at.saturating_sub(started_at),
        started_at,
        ended_at,
    })
}

pub(crate) fn store_terminal_run(db: &Connection, run: &TerminalRun) -> Result<(), String> {
    db.execute(
        "INSERT INTO terminal_sessions (id, workspace_id, kind, label, command, cwd, output, exit_code, checkpoint_id, started_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            run.id,
            run.workspace_id,
            run.kind,
            run.label,
            run.command,
            run.cwd,
            run.output,
            run.exit_code,
            run.checkpoint_id,
            run.started_at,
            run.ended_at
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub(crate) fn list_pulse_evidence_for_db(
    db: &Connection,
    workspace_id: &str,
    limit: i64,
) -> Result<Vec<TerminalRun>, String> {
    let limit = limit.clamp(1, 50);
    let mut stmt = db
        .prepare(
            "SELECT id, workspace_id, kind, label, command, cwd, output, exit_code, checkpoint_id, started_at, ended_at
             FROM terminal_sessions
             WHERE workspace_id = ?1
             ORDER BY ended_at DESC, started_at DESC
             LIMIT ?2",
        )
        .map_err(|err| err.to_string())?;
    let rows = stmt
        .query_map(params![workspace_id, limit], |row| {
            let started_at = row.get::<_, i64>(9)?;
            let ended_at = row.get::<_, i64>(10)?;
            let kind = normalized_pulse_kind(&row.get::<_, String>(2)?).to_string();
            let command = row.get::<_, String>(4)?;
            let label = pulse_label(&kind, &row.get::<_, String>(3)?, &command);
            Ok(TerminalRun {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                kind,
                label,
                command,
                cwd: row.get(5)?,
                output: row.get(6)?,
                exit_code: row.get(7)?,
                checkpoint_id: row.get(8)?,
                duration_ms: ended_at.saturating_sub(started_at),
                started_at,
                ended_at,
            })
        })
        .map_err(|err| err.to_string())?;
    let mut runs = Vec::new();
    for row in rows {
        runs.push(row.map_err(|err| err.to_string())?);
    }
    Ok(runs)
}

pub(crate) fn named_pulses_for_path(cwd: &str) -> Result<Vec<NamedPulse>, String> {
    let root = Path::new(cwd);
    let mut pulses = Vec::new();

    if root.join("package.json").is_file() {
        let scripts = package_scripts(root)?;
        let runner = package_script_runner(root);
        add_package_pulse(
            &mut pulses,
            &scripts,
            &runner,
            "test",
            "Tests",
            "Run the project's test script",
            &["test", "tests", "test:unit"],
        );
        add_package_pulse(
            &mut pulses,
            &scripts,
            &runner,
            "typecheck",
            "Type check",
            "Run the project's type checker",
            &["typecheck", "type-check", "check:types", "tsc"],
        );
        add_package_pulse(
            &mut pulses,
            &scripts,
            &runner,
            "lint",
            "Lint",
            "Run the project's lint script",
            &["lint", "lint:check"],
        );
        add_package_pulse(
            &mut pulses,
            &scripts,
            &runner,
            "build",
            "Build",
            "Run the project's build script",
            &["build"],
        );
        add_package_pulse(
            &mut pulses,
            &scripts,
            &runner,
            "audit",
            "Audit",
            "Run the project's dependency audit script",
            &["audit"],
        );
        if pulses.iter().all(|pulse| pulse.id != "audit") {
            let command = match runner.as_str() {
                "pnpm" => Some("pnpm audit".to_string()),
                _ if root.join("package-lock.json").is_file() => {
                    Some("npm audit --audit-level=moderate".to_string())
                }
                _ => None,
            };
            if let Some(command) = command {
                pulses.push(NamedPulse {
                    id: "audit".to_string(),
                    title: "Audit".to_string(),
                    command,
                    detail: "Check package dependency advisories".to_string(),
                    source: "package manager".to_string(),
                });
            }
        }
    }

    add_cargo_pulses(&mut pulses, root);
    Ok(pulses)
}

pub(crate) fn command_label(command: &str) -> String {
    let first_line = command
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Command");
    let mut label = first_line.trim().to_string();
    if label.chars().count() > 72 {
        label = label.chars().take(69).collect::<String>() + "...";
    }
    label
}

fn package_scripts(root: &Path) -> Result<HashMap<String, String>, String> {
    let content =
        std::fs::read_to_string(root.join("package.json")).map_err(|err| err.to_string())?;
    let value: Value = serde_json::from_str(&content).map_err(|err| err.to_string())?;
    let scripts = value
        .get("scripts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(scripts
        .into_iter()
        .filter_map(|(name, value)| value.as_str().map(|script| (name, script.to_string())))
        .collect())
}

fn package_script_runner(root: &Path) -> String {
    if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        "bun".to_string()
    } else if root.join("pnpm-lock.yaml").is_file() {
        "pnpm".to_string()
    } else if root.join("yarn.lock").is_file() {
        "yarn".to_string()
    } else {
        "npm".to_string()
    }
}

fn add_package_pulse(
    pulses: &mut Vec<NamedPulse>,
    scripts: &HashMap<String, String>,
    runner: &str,
    id: &str,
    title: &str,
    detail: &str,
    candidates: &[&str],
) {
    if pulses.iter().any(|pulse| pulse.id == id) {
        return;
    }
    if let Some(script_name) = candidates
        .iter()
        .find(|candidate| scripts.contains_key(**candidate))
    {
        pulses.push(NamedPulse {
            id: id.to_string(),
            title: title.to_string(),
            command: package_run_command(runner, script_name),
            detail: detail.to_string(),
            source: format!("package.json:{script_name}"),
        });
    }
}

fn package_run_command(runner: &str, script_name: &str) -> String {
    match runner {
        "bun" => format!("bun run {script_name}"),
        "pnpm" => format!("pnpm run {script_name}"),
        "yarn" => format!("yarn run {script_name}"),
        _ => format!("npm run {script_name}"),
    }
}

fn add_cargo_pulses(pulses: &mut Vec<NamedPulse>, root: &Path) {
    let manifest = if root.join("Cargo.toml").is_file() {
        Some("Cargo.toml")
    } else if root.join("src-tauri").join("Cargo.toml").is_file() {
        Some("src-tauri/Cargo.toml")
    } else {
        None
    };
    if let Some(manifest) = manifest {
        if pulses.iter().all(|pulse| pulse.id != "test") {
            pulses.push(NamedPulse {
                id: "test".to_string(),
                title: "Tests".to_string(),
                command: cargo_command("test", manifest),
                detail: "Run Cargo tests".to_string(),
                source: manifest.to_string(),
            });
        }
        if pulses.iter().all(|pulse| pulse.id != "build") {
            pulses.push(NamedPulse {
                id: "build".to_string(),
                title: "Build".to_string(),
                command: cargo_command("build", manifest),
                detail: "Build the Rust target".to_string(),
                source: manifest.to_string(),
            });
        }
    }
}

fn cargo_command(action: &str, manifest: &str) -> String {
    if manifest == "Cargo.toml" {
        format!("cargo {action}")
    } else {
        format!("cargo {action} --manifest-path {manifest}")
    }
}

fn normalized_pulse_kind(kind: &str) -> &'static str {
    match kind {
        "setup" => "setup",
        "run" => "run",
        "pulse" => "pulse",
        _ => "command",
    }
}

fn pulse_label(kind: &str, label: &str, command: &str) -> String {
    let label = label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    match normalized_pulse_kind(kind) {
        "setup" => "Setup script".to_string(),
        "run" => "Run script".to_string(),
        "pulse" => "Pulse".to_string(),
        _ => command_label(command),
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_pulses_use_project_scripts_and_cargo_fallbacks(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!("loomen-pulses-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src-tauri"))?;
        std::fs::write(
            root.join("package.json"),
            r#"{
              "scripts": {
                "build": "cargo build --manifest-path src-tauri/Cargo.toml",
                "lint": "echo lint"
              }
            }"#,
        )?;
        std::fs::write(
            root.join("src-tauri").join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;

        let pulses = named_pulses_for_path(root.to_str().unwrap())?;
        assert!(pulses.iter().any(|pulse| pulse.id == "test"
            && pulse.command == "cargo test --manifest-path src-tauri/Cargo.toml"));
        assert!(pulses
            .iter()
            .any(|pulse| pulse.id == "build" && pulse.command == "npm run build"));
        assert!(pulses
            .iter()
            .any(|pulse| pulse.id == "lint" && pulse.command == "npm run lint"));
        assert!(pulses.iter().all(|pulse| !pulse.title.is_empty()));
        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }
}
