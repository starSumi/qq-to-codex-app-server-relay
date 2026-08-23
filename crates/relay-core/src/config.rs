use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use path_clean::PathClean;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationMode {
    Safe,
    Dangerous,
}

impl AuthorizationMode {
    fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value
            .unwrap_or("dangerous")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "safe" => Ok(Self::Safe),
            "dangerous" => Ok(Self::Dangerous),
            other => anyhow::bail!("RELAY_AUTH_MODE must be safe or dangerous, got {other}"),
        }
    }

    fn requires_allowlist(self) -> bool {
        matches!(self, Self::Safe)
    }

    pub fn is_dangerous(self) -> bool {
        matches!(self, Self::Dangerous)
    }

    pub fn authorizes(
        self,
        sender_is_bot: bool,
        sender_id: &str,
        allow_from: &HashSet<String>,
    ) -> bool {
        if sender_is_bot {
            return false;
        }
        match self {
            Self::Safe => allow_from.contains(sender_id),
            Self::Dangerous => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl SandboxMode {
    pub fn app_server_policy(self, workspace: &Path, network_access: bool) -> serde_json::Value {
        match self {
            Self::DangerFullAccess => serde_json::json!({
                "type": "dangerFullAccess"
            }),
            Self::ReadOnly => serde_json::json!({
                "type": "readOnly",
                "networkAccess": network_access
            }),
            Self::WorkspaceWrite => serde_json::json!({
                "type": "workspaceWrite",
                "networkAccess": network_access,
                "writableRoots": [workspace]
            }),
        }
    }

    pub fn app_server_mode(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub auth_mode: AuthorizationMode,
    pub allow_from: HashSet<String>,
    pub codex_bin: String,
    pub codex_home: PathBuf,
    pub workspace: PathBuf,
    pub model: Option<String>,
    pub sandbox: SandboxMode,
    pub network_access: bool,
    pub allow_external_tools: bool,
    pub turn_timeout: Duration,
    pub database: PathBuf,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_concurrent_turns: usize,
    pub max_queue: usize,
    pub rate_limit_per_minute: usize,
    pub retention: Duration,
}

fn boolean(name: &str, fallback: bool) -> anyhow::Result<bool> {
    match env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("") => Ok(fallback),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => anyhow::bail!("{name} must be true or false"),
    }
}

fn required(name: &str) -> anyhow::Result<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing required environment variable {name}"))
}

fn parse_allow_from(
    value: Option<&str>,
    auth_mode: AuthorizationMode,
) -> anyhow::Result<HashSet<String>> {
    let allow_from = value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<HashSet<_>>();
    if auth_mode.requires_allowlist() && allow_from.is_empty() {
        anyhow::bail!("QQBOT_ALLOW_FROM is required when RELAY_AUTH_MODE=safe");
    }
    if auth_mode.requires_allowlist() && allow_from.contains("*") {
        anyhow::bail!(
            "QQBOT_ALLOW_FROM must contain explicit QQ openids when RELAY_AUTH_MODE=safe"
        );
    }
    if allow_from
        .iter()
        .any(|value| value.len() > 256 || value.chars().any(char::is_control))
    {
        anyhow::bail!("QQBOT_ALLOW_FROM contains an invalid QQ openid");
    }
    Ok(allow_from)
}

fn positive_usize(
    name: &str,
    fallback: usize,
    minimum: usize,
    maximum: usize,
) -> anyhow::Result<usize> {
    let value = env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map_or_else(
            || Ok(fallback),
            |value| {
                value
                    .parse::<usize>()
                    .with_context(|| format!("{name} must be an integer"))
            },
        )?;
    if !(minimum..=maximum).contains(&value) {
        anyhow::bail!("{name} must be between {minimum} and {maximum}");
    }
    Ok(value)
}

fn parse_sandbox(value: Option<&str>) -> anyhow::Result<SandboxMode> {
    match value.unwrap_or("danger-full-access").trim() {
        "read-only" => Ok(SandboxMode::ReadOnly),
        "workspace-write" => Ok(SandboxMode::WorkspaceWrite),
        "danger-full-access" => Ok(SandboxMode::DangerFullAccess),
        other => anyhow::bail!(
            "unsupported CODEX_SANDBOX {other}; only read-only, workspace-write, and danger-full-access are allowed"
        ),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let auth_mode = AuthorizationMode::parse(env::var("RELAY_AUTH_MODE").ok().as_deref())?;
        let allow_from = parse_allow_from(env::var("QQBOT_ALLOW_FROM").ok().as_deref(), auth_mode)?;

        let approval = env::var("CODEX_APPROVAL_POLICY").unwrap_or_else(|_| "never".into());
        if approval.trim() != "never" {
            anyhow::bail!(
                "CODEX_APPROVAL_POLICY must be never because the QQ adapter has no approval UI"
            );
        }

        let sandbox = parse_sandbox(env::var("CODEX_SANDBOX").ok().as_deref())?;

        let codex_home_input = PathBuf::from(required("CODEX_HOME")?);
        if !codex_home_input.is_dir() {
            anyhow::bail!("CODEX_HOME must be an existing directory");
        }
        let codex_home =
            std::fs::canonicalize(&codex_home_input).context("cannot resolve CODEX_HOME")?;
        let workspace_input = PathBuf::from(required("CODEX_WORKSPACE")?);
        if !workspace_input.is_dir() {
            anyhow::bail!("CODEX_WORKSPACE must be an existing directory");
        }
        let workspace =
            std::fs::canonicalize(&workspace_input).context("cannot resolve CODEX_WORKSPACE")?;
        if codex_home.starts_with(&workspace) || workspace.starts_with(&codex_home) {
            anyhow::bail!("CODEX_HOME must be outside CODEX_WORKSPACE");
        }

        let timeout_ms = positive_usize("CODEX_TURN_TIMEOUT_MS", 600_000, 1_000, 3_600_000)?;
        let database_input = env::var("RELAY_DB")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map_or_else(|| PathBuf::from("state/relay.sqlite3"), PathBuf::from);
        let database = if database_input.is_absolute() {
            database_input
        } else {
            env::current_dir()
                .context("cannot resolve the relay working directory")?
                .join(database_input)
        }
        .clean();
        if database.starts_with(&workspace) {
            anyhow::bail!("RELAY_DB must be outside CODEX_WORKSPACE");
        }

        Ok(Self {
            auth_mode,
            allow_from,
            codex_bin: env::var("CODEX_BIN")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "codex".into()),
            codex_home,
            workspace,
            model: env::var("CODEX_MODEL")
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            sandbox,
            network_access: boolean("CODEX_NETWORK_ACCESS", true)?,
            allow_external_tools: boolean("CODEX_ALLOW_EXTERNAL_TOOLS", true)?,
            turn_timeout: Duration::from_millis(timeout_ms as u64),
            database,
            max_input_bytes: positive_usize("RELAY_MAX_INPUT_BYTES", 16_384, 1, 65_536)?,
            max_output_bytes: positive_usize("RELAY_MAX_OUTPUT_BYTES", 32_768, 1, 262_144)?,
            max_concurrent_turns: positive_usize("RELAY_MAX_CONCURRENT_TURNS", 2, 1, 8)?,
            max_queue: positive_usize("RELAY_MAX_QUEUE", 64, 1, 1_024)?,
            rate_limit_per_minute: positive_usize("RELAY_RATE_LIMIT_PER_MINUTE", 6, 1, 120)?,
            retention: Duration::from_secs(
                u64::try_from(positive_usize("RELAY_RETENTION_DAYS", 30, 1, 3_650)?)? * 86_400,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_mode_defaults_to_qr_dangerous() {
        assert_eq!(
            AuthorizationMode::parse(None).unwrap(),
            AuthorizationMode::Dangerous
        );
        assert_eq!(
            AuthorizationMode::parse(Some("DANGEROUS")).unwrap(),
            AuthorizationMode::Dangerous
        );
        assert!(AuthorizationMode::parse(Some("open")).is_err());
    }

    #[test]
    fn safe_mode_requires_explicit_allowlist_entries() {
        assert!(parse_allow_from(None, AuthorizationMode::Safe).is_err());
        assert!(parse_allow_from(Some("*"), AuthorizationMode::Safe).is_err());
        let entries = parse_allow_from(Some("alice, bob"), AuthorizationMode::Safe).unwrap();
        assert!(entries.contains("alice"));
        assert!(entries.contains("bob"));
    }

    #[test]
    fn sandbox_policy_has_no_network_access() {
        let read_only = SandboxMode::ReadOnly.app_server_policy(Path::new("workspace"), false);
        let workspace_write =
            SandboxMode::WorkspaceWrite.app_server_policy(Path::new("workspace"), false);
        assert_eq!(read_only["networkAccess"], false);
        assert_eq!(workspace_write["networkAccess"], false);
    }

    #[test]
    fn danger_full_access_policy_is_unrestricted() {
        let policy = SandboxMode::DangerFullAccess.app_server_policy(Path::new("workspace"), false);
        assert_eq!(policy["type"], "dangerFullAccess");
        assert!(policy.get("networkAccess").is_none());
    }

    #[test]
    fn sandbox_policy_can_enable_network_access() {
        let policy = SandboxMode::WorkspaceWrite.app_server_policy(Path::new("workspace"), true);
        assert_eq!(policy["networkAccess"], true);
    }

    #[test]
    fn sandbox_default_allows_full_access() {
        assert_eq!(parse_sandbox(None).unwrap(), SandboxMode::DangerFullAccess);
        assert_eq!(
            parse_sandbox(Some("read-only")).unwrap(),
            SandboxMode::ReadOnly
        );
        assert_eq!(
            parse_sandbox(Some("danger-full-access")).unwrap(),
            SandboxMode::DangerFullAccess
        );
        assert!(parse_sandbox(Some("invalid")).is_err());
    }

    #[test]
    fn boolean_values_are_strict() {
        assert!(boolean("RELAY_TEST_BOOLEAN_NOT_SET", false).is_ok_and(|value| !value));
    }
}
