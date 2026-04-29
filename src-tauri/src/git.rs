use std::path::Path;
use std::process::{Command, Stdio};
use uuid::Uuid;

pub(crate) fn branch_exists_for_worktree(path: &str, branch: &str) -> Option<bool> {
    if !Path::new(path).exists() {
        return None;
    }
    Command::new("git")
        .arg("show-ref")
        .arg("--verify")
        .arg("--quiet")
        .arg(format!("refs/heads/{branch}"))
        .current_dir(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .map(|status| status.success())
}

pub(crate) fn resolve_git_root(path: &str) -> Result<String, String> {
    let expanded = expand_tilde(path);
    git_output(&expanded, &["rev-parse", "--show-toplevel"])
}

pub(crate) fn detect_default_branch(repo_path: &str) -> Option<String> {
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

pub(crate) fn list_git_branches(
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

pub(crate) fn create_git_worktree(
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

pub(crate) fn save_checkpoint(worktree_path: &str, id: &str) -> Result<String, String> {
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

pub(crate) fn checkpoint_diff(worktree_path: &str, id: &str, mode: &str) -> Result<String, String> {
    let ref_name = checkpoint_ref(&sanitize_checkpoint_id(id)?)?;
    if mode == "--stat" {
        git_output(worktree_path, &["diff", &ref_name, "--stat"])
    } else {
        git_output(worktree_path, &["diff", &ref_name])
    }
}

pub(crate) fn git_output(repo_path: &str, args: &[&str]) -> Result<String, String> {
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

fn git_status(repo_path: &str, args: &[&str]) -> Result<(), String> {
    git_output(repo_path, args).map(|_| ())
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

fn push_unique_branch(branches: &mut Vec<String>, branch: &str) {
    let branch = branch.trim();
    if branch.is_empty() || branches.iter().any(|item| item == branch) {
        return;
    }
    branches.push(branch.to_string());
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        std::env::var("HOME").unwrap_or_else(|_| path.to_string())
    } else if let Some(rest) = path.strip_prefix("~/") {
        std::env::var("HOME")
            .map(|home| format!("{home}/{rest}"))
            .unwrap_or_else(|_| path.to_string())
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_ids_accept_only_ref_safe_values() {
        assert_eq!(
            sanitize_checkpoint_id(" workspace-abc_123.4 ").unwrap(),
            "workspace-abc_123.4"
        );
        assert_eq!(
            checkpoint_ref("workspace/abc").unwrap(),
            "refs/loomen-checkpoints/workspace/abc"
        );

        for id in [
            "",
            "   ",
            "../escape",
            "workspace/../escape",
            "-bad",
            "bad value",
        ] {
            assert!(
                sanitize_checkpoint_id(id).is_err(),
                "{id} should be rejected"
            );
            assert!(checkpoint_ref(id).is_err(), "{id} should not build a ref");
        }
    }

    #[test]
    fn branch_listing_prefers_current_default_and_unique_refs(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp_root = std::env::temp_dir().join(format!("loomen-git-{}", Uuid::new_v4()));
        let repo = temp_root.join("repo");
        std::fs::create_dir_all(&repo)?;
        init_repo(&repo)?;
        run_command("git", &["branch", "feature/ray"], &repo)?;

        let branches = list_git_branches(repo.to_str().unwrap(), Some("feature/ray"), Some("main"));
        assert_eq!(branches[0], "feature/ray");
        assert_eq!(branches[1], "main");
        assert_eq!(branches[2], "HEAD");
        assert_eq!(
            branches
                .iter()
                .filter(|branch| branch.as_str() == "feature/ray")
                .count(),
            1
        );

        let _ = std::fs::remove_dir_all(temp_root);
        Ok(())
    }

    #[test]
    fn creates_real_git_worktree_and_checkpoint_ref() -> Result<(), Box<dyn std::error::Error>> {
        let temp_root = std::env::temp_dir().join(format!("loomen-git-{}", Uuid::new_v4()));
        let repo = temp_root.join("repo");
        let worktree = temp_root.join("worktree");
        std::fs::create_dir_all(&repo)?;
        init_repo(&repo)?;

        create_git_worktree(
            repo.to_str().unwrap(),
            worktree.to_str().unwrap(),
            "loomen/test-worktree",
            "main",
        )?;
        assert!(worktree.join("README.md").exists());
        assert_eq!(
            branch_exists_for_worktree(worktree.to_str().unwrap(), "loomen/test-worktree"),
            Some(true)
        );

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
        let _ = std::fs::remove_dir_all(temp_root);
        Ok(())
    }

    fn init_repo(repo: &Path) -> Result<(), Box<dyn std::error::Error>> {
        run_command("git", &["init", "-b", "main"], repo)?;
        run_command("git", &["config", "user.name", "Loomen Test"], repo)?;
        run_command(
            "git",
            &["config", "user.email", "loomen-test@example.invalid"],
            repo,
        )?;
        std::fs::write(repo.join("README.md"), "hello\n")?;
        run_command("git", &["add", "README.md"], repo)?;
        run_command("git", &["commit", "-m", "initial"], repo)?;
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
