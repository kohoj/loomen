use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

const PR_INFO_FIELDS: &str =
    "number,title,url,state,isDraft,headRefName,baseRefName,statusCheckRollup";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestInfo {
    pub(crate) workspace_id: String,
    pub(crate) number: Option<i64>,
    pub(crate) title: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) is_draft: bool,
    pub(crate) head_ref_name: Option<String>,
    pub(crate) base_ref_name: Option<String>,
    pub(crate) checks: Vec<CheckInfo>,
    pub(crate) error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckInfo {
    pub(crate) name: String,
    pub(crate) kind: Option<String>,
    pub(crate) workflow_name: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) conclusion: Option<String>,
    pub(crate) details_url: Option<String>,
    pub(crate) started_at: Option<String>,
    pub(crate) completed_at: Option<String>,
}

pub(crate) fn get_pull_request_info_for_cwd(
    workspace_id: &str,
    cwd: &str,
) -> Result<PullRequestInfo, String> {
    let output = gh_command(cwd)
        .arg("pr")
        .arg("view")
        .arg("--json")
        .arg(PR_INFO_FIELDS)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        parse_pr_info_json(workspace_id, &String::from_utf8_lossy(&output.stdout))
    } else {
        Ok(PullRequestInfo::error(
            workspace_id,
            None,
            None,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

pub(crate) fn create_pull_request_for_cwd(
    workspace_id: &str,
    cwd: &str,
    branch_name: Option<String>,
    base_branch: Option<String>,
    title: &str,
    body: &str,
    draft: bool,
) -> Result<PullRequestInfo, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("PR title is required".to_string());
    }
    let mut command = gh_command(cwd);
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
        });
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
        return Ok(PullRequestInfo::error(
            workspace_id,
            branch_name,
            base_branch,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let view = gh_command(cwd)
        .arg("pr")
        .arg("view")
        .arg("--json")
        .arg(PR_INFO_FIELDS)
        .output()
        .map_err(|err| err.to_string())?;
    if view.status.success() {
        parse_pr_info_json(workspace_id, &String::from_utf8_lossy(&view.stdout))
    } else {
        Ok(PullRequestInfo {
            workspace_id: workspace_id.to_string(),
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

pub(crate) fn update_pull_request_for_cwd(
    workspace_id: &str,
    cwd: &str,
    title: &str,
    body: &str,
) -> Result<PullRequestInfo, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("PR title is required".to_string());
    }
    let output = gh_command(cwd)
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
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Ok(PullRequestInfo::error(
            workspace_id,
            None,
            None,
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    get_pull_request_info_for_cwd(workspace_id, cwd)
}

pub(crate) fn rerun_failed_checks_for_branch(cwd: &str, branch: &str) -> Result<String, String> {
    let list_output = gh_command(cwd)
        .arg("run")
        .arg("list")
        .arg("--branch")
        .arg(branch)
        .arg("--limit")
        .arg("1")
        .arg("--json")
        .arg("databaseId,status,conclusion,name")
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
    let rerun_output = gh_command(cwd)
        .arg("run")
        .arg("rerun")
        .arg(run_id.to_string())
        .arg("--failed")
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

fn check_string_field(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        item.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

fn gh_command(cwd: &str) -> Command {
    let mut command = Command::new(gh_path());
    command
        .current_dir(cwd)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    command
}

fn gh_path() -> PathBuf {
    PathBuf::from("gh")
}

impl PullRequestInfo {
    fn error(
        workspace_id: &str,
        head_ref_name: Option<String>,
        base_ref_name: Option<String>,
        error: String,
    ) -> Self {
        Self {
            workspace_id: workspace_id.to_string(),
            number: None,
            title: None,
            url: None,
            state: None,
            is_draft: false,
            head_ref_name,
            base_ref_name,
            checks: Vec::new(),
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn pr_check_field_reader_ignores_blank_strings() -> Result<(), Box<dyn std::error::Error>> {
        let raw = r#"{
          "statusCheckRollup": [
            {
              "name": "   ",
              "context": " fallback ",
              "detailsUrl": "",
              "targetUrl": " https://ci.example.invalid/fallback "
            }
          ]
        }"#;
        let info = parse_pr_info_json("workspace-1", raw)?;
        assert_eq!(info.checks[0].name, "fallback");
        assert_eq!(
            info.checks[0].details_url.as_deref(),
            Some("https://ci.example.invalid/fallback")
        );
        Ok(())
    }
}
