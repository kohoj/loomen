use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppSettings {
    pub(crate) send_messages_with: String,
    pub(crate) desktop_notifications: bool,
    pub(crate) sound_effects: bool,
    pub(crate) auto_convert_long_text: bool,
    pub(crate) strip_absolute_right: bool,
    pub(crate) always_show_context_usage: bool,
    pub(crate) expand_tool_calls: bool,
    pub(crate) default_claude_model: String,
    pub(crate) default_codex_model: String,
    pub(crate) default_codex_effort: String,
    pub(crate) codex_personality: String,
    pub(crate) review_model: String,
    pub(crate) review_codex_effort: String,
    pub(crate) default_to_plan_mode: bool,
    pub(crate) default_to_fast_mode: bool,
    pub(crate) claude_chrome: bool,
    pub(crate) provider_env: String,
    pub(crate) codex_provider_mode: String,
    pub(crate) theme: String,
    pub(crate) colored_sidebar_diffs: bool,
    pub(crate) mono_font: String,
    pub(crate) markdown_style: String,
    pub(crate) terminal_font: String,
    pub(crate) terminal_font_size: i64,
    pub(crate) branch_prefix_type: String,
    pub(crate) branch_prefix_custom: String,
    pub(crate) delete_branch_on_archive: bool,
    pub(crate) archive_on_merge: bool,
    pub(crate) loomen_root_directory: String,
    pub(crate) claude_executable_path: String,
    pub(crate) codex_executable_path: String,
    pub(crate) big_terminal_mode: bool,
    pub(crate) dashboard: bool,
    pub(crate) voice_mode: bool,
    pub(crate) automerge: bool,
    pub(crate) spotlight_testing: bool,
    pub(crate) sidebar_resource_usage: bool,
    pub(crate) match_workspace_directory_with_branch_name: bool,
    pub(crate) experimental_terminal_runtime: bool,
    pub(crate) react_profiler: bool,
    pub(crate) enterprise_data_privacy: bool,
    pub(crate) claude_tool_approvals: bool,
}

pub(crate) fn load_settings(db: &Connection) -> anyhow::Result<AppSettings> {
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

pub(crate) fn save_settings(db: &Connection, settings: &AppSettings) -> anyhow::Result<()> {
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

pub(crate) fn default_model_for_agent(settings: &AppSettings, agent_type: &str) -> String {
    if agent_type == "codex" {
        non_empty(&settings.default_codex_model, "gpt-5-codex")
    } else {
        non_empty(&settings.default_claude_model, "opus")
    }
}

pub(crate) fn default_permission_mode(settings: &AppSettings) -> &'static str {
    if settings.default_to_plan_mode {
        "plan"
    } else if settings.default_to_fast_mode {
        "dontAsk"
    } else {
        "default"
    }
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

fn non_empty(value: &str, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_preserve_loomen_runtime_posture() -> Result<(), Box<dyn std::error::Error>>
    {
        let db = settings_db()?;
        let settings = load_settings(&db)?;

        assert_eq!(settings.send_messages_with, "Enter");
        assert_eq!(settings.default_claude_model, "opus");
        assert_eq!(settings.default_codex_model, "gpt-5-codex");
        assert_eq!(settings.default_codex_effort, "high");
        assert_eq!(settings.codex_provider_mode, "cli");
        assert_eq!(settings.branch_prefix_type, "github_username");
        assert!(settings.default_to_plan_mode);
        assert!(!settings.default_to_fast_mode);
        assert!(settings.loomen_root_directory.ends_with("/loomen"));
        assert_eq!(default_model_for_agent(&settings, "codex"), "gpt-5-codex");
        assert_eq!(default_model_for_agent(&settings, "claude"), "opus");
        assert_eq!(default_permission_mode(&settings), "plan");
        Ok(())
    }

    #[test]
    fn settings_round_trip_through_sqlite() -> Result<(), Box<dyn std::error::Error>> {
        let db = settings_db()?;
        let mut settings = load_settings(&db)?;
        settings.send_messages_with = "Cmd+Enter".to_string();
        settings.default_codex_model = "gpt-5.2".to_string();
        settings.default_to_plan_mode = false;
        settings.default_to_fast_mode = true;
        settings.terminal_font_size = 15;
        settings.branch_prefix_type = "custom".to_string();
        settings.branch_prefix_custom = "loomen-lab".to_string();
        settings.loomen_root_directory = "/tmp/loomen-work".to_string();
        settings.claude_tool_approvals = true;

        save_settings(&db, &settings)?;
        let loaded = load_settings(&db)?;

        assert_eq!(loaded.send_messages_with, "Cmd+Enter");
        assert_eq!(loaded.default_codex_model, "gpt-5.2");
        assert_eq!(loaded.terminal_font_size, 15);
        assert_eq!(loaded.branch_prefix_type, "custom");
        assert_eq!(loaded.branch_prefix_custom, "loomen-lab");
        assert_eq!(loaded.loomen_root_directory, "/tmp/loomen-work");
        assert!(loaded.claude_tool_approvals);
        assert_eq!(default_permission_mode(&loaded), "dontAsk");
        Ok(())
    }

    #[test]
    fn setting_parsers_tolerate_human_values_and_bad_numbers(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let db = settings_db()?;
        put_setting(&db, "desktop_notifications", "YES")?;
        put_setting(&db, "sound_effects", "0")?;
        put_setting(&db, "terminal_font_size", "large")?;

        assert!(setting_bool(&db, "desktop_notifications", false)?);
        assert!(!setting_bool(&db, "sound_effects", true)?);
        assert_eq!(setting_i64(&db, "terminal_font_size", 12)?, 12);
        Ok(())
    }

    fn settings_db() -> Result<Connection, Box<dyn std::error::Error>> {
        let db = Connection::open_in_memory()?;
        db.execute(
            "CREATE TABLE settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;
        Ok(db)
    }
}
