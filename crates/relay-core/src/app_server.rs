use std::{
    collections::HashMap,
    env,
    ffi::{OsStr, OsString},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::Context;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, oneshot},
    time::{self, Duration},
};

use crate::config::{Config, SandboxMode};

const EXTERNAL_TOOL_FEATURES: &[&str] = &[
    "auth_elicitation",
    "hooks",
    "plugins",
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "remote_control",
    "multi_agent",
    "multi_agent_v2",
    "code_mode",
    "goals",
    "js_repl",
    "mcp_2026_07_28",
    "memories",
    "remote_plugin",
    "tool_suggest",
];

#[derive(Debug, thiserror::Error)]
pub enum AppServerError {
    #[error("app-server request failed: {0}")]
    Rpc(String),
    #[error("app-server transport closed")]
    Closed,
    #[error("app-server turn timed out")]
    Timeout,
    #[error("app-server request timed out: {0}")]
    RequestTimeout(String),
    #[error("app-server returned an invalid response: {0}")]
    Protocol(String),
}

impl AppServerError {
    pub(crate) fn failure_kind(&self) -> &'static str {
        match self {
            Self::Rpc(message) if has_http_status(message, "502", "Bad Gateway") => {
                "upstream_bad_gateway"
            }
            Self::Rpc(message) if has_http_status(message, "503", "Service Unavailable") => {
                "upstream_unavailable"
            }
            Self::Rpc(message) if has_http_status(message, "429", "Too Many Requests") => {
                "upstream_rate_limited"
            }
            Self::Rpc(message)
                if has_http_status(message, "401", "Unauthorized")
                    || has_http_status(message, "403", "Forbidden") =>
            {
                "upstream_auth_failed"
            }
            Self::Rpc(_) => "upstream_request_failed",
            Self::Closed => "transport_closed",
            Self::Timeout => "turn_timeout",
            Self::RequestTimeout(_) => "request_timeout",
            Self::Protocol(_) => "protocol_error",
        }
    }
}

fn has_http_status(message: &str, status: &str, reason: &str) -> bool {
    message.split_whitespace().any(|token| {
        token.trim_matches(|character: char| !character.is_ascii_alphanumeric()) == status
    }) || message.contains(reason)
}

struct TurnState {
    text: String,
    turn_id: Option<String>,
    completion: Option<oneshot::Sender<Result<String, AppServerError>>>,
}

struct Inner {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, AppServerError>>>>,
    turns: Mutex<HashMap<String, TurnState>>,
    next_id: AtomicU64,
    output_buffer_bytes: usize,
}

#[derive(Clone)]
pub struct AppServerClient {
    inner: Arc<Inner>,
    workspace: std::path::PathBuf,
    model: Option<String>,
    sandbox: SandboxMode,
    network_access: bool,
    timeout: Duration,
}

fn keep_environment_key(key: &OsStr) -> bool {
    let upper = key.to_string_lossy().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "PATH"
            | "PATHEXT"
            | "SYSTEMROOT"
            | "WINDIR"
            | "COMSPEC"
            | "TEMP"
            | "TMP"
            | "TMPDIR"
            | "HOME"
            | "USERPROFILE"
            | "LOCALAPPDATA"
            | "APPDATA"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "NO_PROXY"
            | "SSL_CERT_FILE"
            | "SSL_CERT_DIR"
            | "LANG"
            | "LC_ALL"
            | "TERM"
            | "NO_COLOR"
    )
}

fn child_environment(codex_home: &OsStr) -> Vec<(OsString, OsString)> {
    let mut values = env::vars_os()
        .filter(|(key, _)| keep_environment_key(key))
        .collect::<Vec<_>>();
    values.push((OsString::from("CODEX_HOME"), codex_home.to_os_string()));
    values
}

impl AppServerClient {
    pub async fn start(config: &Config) -> anyhow::Result<Self> {
        let mut command = Command::new(&config.codex_bin);
        command
            .args(codex_arguments(
                config.allow_external_tools,
                &config.codex_home,
            )?)
            .current_dir(&config.workspace)
            .env_clear()
            .envs(child_environment(config.codex_home.as_os_str()))
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);

        let mut child = command
            .spawn()
            .with_context(|| format!("cannot start Codex app-server with {}", config.codex_bin))?;
        let stdin = child
            .stdin
            .take()
            .context("app-server stdin is unavailable")?;
        let stdout = child
            .stdout
            .take()
            .context("app-server stdout is unavailable")?;
        let stderr = child
            .stderr
            .take()
            .context("app-server stderr is unavailable")?;
        let inner = Arc::new(Inner {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            output_buffer_bytes: config.max_output_bytes.saturating_add(4),
        });
        let client = Self {
            inner: Arc::clone(&inner),
            workspace: config.workspace.clone(),
            model: config.model.clone(),
            sandbox: config.sandbox,
            network_access: config.network_access,
            timeout: config.turn_timeout,
        };

        tokio::spawn(read_loop(Arc::clone(&inner), stdout));
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => tracing::debug!(bytes = line.len(), "app-server wrote to stderr"),
                }
            }
        });

        let initialized = client
            .request(
                "initialize",
                json!({
                    "clientInfo": {"name": "qq-to-codex-app-server-relay", "version": env!("CARGO_PKG_VERSION")},
                    "capabilities": {"experimentalApi": false}
                }),
            )
            .await;
        if let Err(error) = initialized {
            client.stop().await;
            return Err(error).context("Codex app-server initialize failed");
        }
        if let Err(error) = client.notify("initialized", json!({})).await {
            client.stop().await;
            return Err(error).context("Codex app-server initialized notification failed");
        }
        if !config.allow_external_tools
            && let Err(error) = client.verify_external_tools_disabled().await
        {
            client.stop().await;
            return Err(error).context("Codex external-tool isolation check failed");
        }
        Ok(client)
    }

    async fn verify_external_tools_disabled(&self) -> Result<(), AppServerError> {
        let config = self
            .request(
                "config/read",
                json!({"includeLayers": false, "cwd": self.workspace}),
            )
            .await?;
        verify_effective_config(&config)?;

        let hooks = self
            .request("hooks/list", json!({"cwds": [self.workspace]}))
            .await?;
        verify_hooks_disabled(&hooks)
    }

    async fn write(&self, value: &Value) -> Result<(), AppServerError> {
        let mut stdin = self.inner.stdin.lock().await;
        let mut bytes = serde_json::to_vec(value)
            .map_err(|error| AppServerError::Protocol(error.to_string()))?;
        bytes.push(b'\n');
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| AppServerError::Closed)?;
        stdin.flush().await.map_err(|_| AppServerError::Closed)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, AppServerError> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, sender);
        if let Err(error) = self
            .write(&json!({"id": id, "method": method, "params": params}))
            .await
        {
            self.inner.pending.lock().await.remove(&id);
            return Err(error);
        }
        if let Ok(result) = time::timeout(Duration::from_secs(30), receiver).await {
            result.unwrap_or(Err(AppServerError::Closed))
        } else {
            self.inner.pending.lock().await.remove(&id);
            Err(AppServerError::RequestTimeout(method.to_owned()))
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), AppServerError> {
        self.write(&json!({"method": method, "params": params}))
            .await
    }

    pub async fn start_thread(&self) -> Result<String, AppServerError> {
        let mut params = json!({
            "cwd": self.workspace,
            "approvalPolicy": "never",
            "sandbox": self.sandbox.app_server_mode(),
            "ephemeral": false,
            "threadSource": "qq-to-codex-app-server-relay"
        });
        if let Some(model) = &self.model {
            params["model"] = Value::String(model.clone());
        }
        let result = self.request("thread/start", params).await?;
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| AppServerError::Protocol("thread/start omitted thread.id".into()))
    }

    pub async fn resume_thread(&self, thread_id: &str) -> Result<String, AppServerError> {
        let result = self
            .request(
                "thread/resume",
                json!({
                    "threadId": thread_id,
                    "cwd": self.workspace,
                    "approvalPolicy": "never",
                    "sandbox": self.sandbox.app_server_mode()
                }),
            )
            .await?;
        Ok(result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .unwrap_or(thread_id)
            .to_owned())
    }

    pub async fn run_turn(&self, thread_id: &str, text: &str) -> Result<String, AppServerError> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut turns = self.inner.turns.lock().await;
            if turns.contains_key(thread_id) {
                return Err(AppServerError::Protocol(
                    "a turn is already active for this thread".into(),
                ));
            }
            turns.insert(
                thread_id.to_owned(),
                TurnState {
                    text: String::new(),
                    turn_id: None,
                    completion: Some(sender),
                },
            );
        }

        let mut params = json!({
            "threadId": thread_id,
            "input": [{"type": "text", "text": text}],
            "approvalPolicy": "never",
            "sandboxPolicy": self
                .sandbox
                .app_server_policy(&self.workspace, self.network_access)
        });
        if let Some(model) = &self.model {
            params["model"] = Value::String(model.clone());
        }
        let started = self.request("turn/start", params).await;
        match started {
            Ok(value) => {
                let Some(turn_id) = value.pointer("/turn/id").and_then(Value::as_str) else {
                    self.inner.turns.lock().await.remove(thread_id);
                    return Err(AppServerError::Protocol(
                        "turn/start omitted turn.id".into(),
                    ));
                };
                if let Some(state) = self.inner.turns.lock().await.get_mut(thread_id) {
                    state.turn_id = Some(turn_id.to_owned());
                }
            }
            Err(error) => {
                self.inner.turns.lock().await.remove(thread_id);
                return Err(error);
            }
        }

        match time::timeout(self.timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppServerError::Closed),
            Err(_) => {
                let state = self.inner.turns.lock().await.remove(thread_id);
                if let Some(turn_id) = state.and_then(|state| state.turn_id) {
                    let _ = self
                        .request(
                            "turn/interrupt",
                            json!({"threadId": thread_id, "turnId": turn_id}),
                        )
                        .await;
                }
                Err(AppServerError::Timeout)
            }
        }
    }

    pub async fn stop(&self) {
        let _ = self.inner.stdin.lock().await.shutdown().await;
        let mut child = self.inner.child.lock().await;
        if time::timeout(Duration::from_secs(3), child.wait())
            .await
            .is_err()
        {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }
}

fn configured_mcp_server_names(codex_home: &Path) -> anyhow::Result<Vec<String>> {
    let config_path = codex_home.join("config.toml");
    let contents = match std::fs::read_to_string(&config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("cannot read Codex user configuration"),
    };
    let document = contents
        .parse::<toml::Table>()
        .context("cannot parse Codex user configuration")?;
    let Some(servers) = document.get("mcp_servers") else {
        return Ok(Vec::new());
    };
    let servers = servers
        .as_table()
        .context("Codex mcp_servers configuration is not a table")?;
    let mut names = servers.keys().cloned().collect::<Vec<_>>();
    names.sort_unstable();
    Ok(names)
}

fn codex_arguments(allow_external_tools: bool, codex_home: &Path) -> anyhow::Result<Vec<String>> {
    let mut arguments = vec![
        "app-server".to_owned(),
        "--stdio".to_owned(),
        "-c".to_owned(),
        "notify=[]".to_owned(),
    ];
    if !allow_external_tools {
        arguments.extend([
            "-c".to_owned(),
            "mcp_servers={}".to_owned(),
            "-c".to_owned(),
            "web_search=\"disabled\"".to_owned(),
        ]);
        for name in configured_mcp_server_names(codex_home)? {
            if name.is_empty()
                || !name.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
            {
                anyhow::bail!("a configured Codex MCP server name cannot be safely overridden");
            }
            arguments.extend(["-c".to_owned(), format!("mcp_servers.{name}.enabled=false")]);
        }
        for feature in EXTERNAL_TOOL_FEATURES {
            arguments.extend(["--disable".to_owned(), (*feature).to_owned()]);
        }
    }
    Ok(arguments)
}

fn verify_effective_config(result: &Value) -> Result<(), AppServerError> {
    let config = result
        .get("config")
        .and_then(Value::as_object)
        .ok_or_else(|| AppServerError::Protocol("config/read omitted config".into()))?;
    let mcp_disabled = match config.get("mcp_servers") {
        None | Some(Value::Null) => true,
        Some(Value::Object(servers)) => servers
            .values()
            .all(|server| server.get("enabled").and_then(Value::as_bool) == Some(false)),
        Some(_) => false,
    };
    if !mcp_disabled {
        return Err(AppServerError::Protocol(
            "an effective MCP server remains enabled".into(),
        ));
    }
    if config.get("web_search").and_then(Value::as_str) != Some("disabled") {
        return Err(AppServerError::Protocol(
            "effective web search mode is not disabled".into(),
        ));
    }
    let features = config
        .get("features")
        .and_then(Value::as_object)
        .ok_or_else(|| AppServerError::Protocol("config/read omitted features".into()))?;
    if EXTERNAL_TOOL_FEATURES
        .iter()
        .any(|feature| features.get(*feature).and_then(Value::as_bool) != Some(false))
    {
        return Err(AppServerError::Protocol(
            "an external-tool feature remains enabled".into(),
        ));
    }
    Ok(())
}

fn verify_hooks_disabled(result: &Value) -> Result<(), AppServerError> {
    let entries = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AppServerError::Protocol("hooks/list omitted data".into()))?;
    for entry in entries {
        let errors = entry
            .get("errors")
            .and_then(Value::as_array)
            .ok_or_else(|| AppServerError::Protocol("hooks/list omitted errors".into()))?;
        if !errors.is_empty() {
            return Err(AppServerError::Protocol(
                "hook discovery returned an error".into(),
            ));
        }
        let hooks = entry
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or_else(|| AppServerError::Protocol("hooks/list omitted hooks".into()))?;
        if hooks
            .iter()
            .any(|hook| hook.get("enabled").and_then(Value::as_bool) != Some(false))
        {
            return Err(AppServerError::Protocol(
                "effective configuration still contains an enabled hook".into(),
            ));
        }
    }
    Ok(())
}

async fn read_loop<R>(inner: Arc<Inner>, output: R)
where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(output).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > 1_048_576 {
            tracing::warn!("app-server emitted an oversized JSONL record");
            break;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            tracing::warn!("app-server emitted invalid JSONL");
            continue;
        };
        handle_message(&inner, message).await;
    }
    fail_all(&inner).await;
}

async fn handle_message(inner: &Arc<Inner>, message: Value) {
    if let Some(id) = message.get("id").and_then(Value::as_u64)
        && (message.get("result").is_some() || message.get("error").is_some())
    {
        if let Some(sender) = inner.pending.lock().await.remove(&id) {
            let result = if let Some(error) = message.get("error") {
                Err(AppServerError::Rpc(
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown app-server error")
                        .to_owned(),
                ))
            } else {
                Ok(message.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ = sender.send(result);
        }
        return;
    }

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    if let Some(id) = message.get("id").cloned() {
        handle_server_request(inner, id, method).await;
        return;
    }
    handle_notification(inner, method, message.get("params").unwrap_or(&Value::Null)).await;
}

async fn handle_server_request(inner: &Arc<Inner>, id: Value, method: &str) {
    let response = server_request_response(&id, method);
    let Ok(mut bytes) = serde_json::to_vec(&response) else {
        return;
    };
    bytes.push(b'\n');
    let mut stdin = inner.stdin.lock().await;
    let _ = stdin.write_all(&bytes).await;
    let _ = stdin.flush().await;
}

fn server_request_response(id: &Value, method: &str) -> Value {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"id": id, "result": {"decision": "decline"}})
        }
        "item/permissions/requestApproval" => json!({
            "id": id,
            "result": {
                "permissions": {"fileSystem": null, "network": null},
                "scope": "turn",
                "strictAutoReview": false
            }
        }),
        "mcpServer/elicitation/request" => {
            json!({"id": id, "result": {"action": "decline"}})
        }
        "item/tool/requestUserInput" => json!({"id": id, "result": {"answers": {}}}),
        _ => {
            json!({"id": id, "error": {"code": -32601, "message": "request is not supported by this relay"}})
        }
    }
}

async fn handle_notification(inner: &Arc<Inner>, method: &str, params: &Value) {
    let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
        return;
    };
    let mut turns = inner.turns.lock().await;
    let Some(state) = turns.get_mut(thread_id) else {
        return;
    };
    match method {
        "item/agentMessage/delta" => {
            if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                append_bounded(&mut state.text, delta, inner.output_buffer_bytes);
            }
        }
        "item/completed" => {
            let item = params.get("item").unwrap_or(&Value::Null);
            if item.get("type").and_then(Value::as_str) == Some("agentMessage")
                && let Some(text) = item.get("text").and_then(Value::as_str)
            {
                state.text.clear();
                append_bounded(&mut state.text, text, inner.output_buffer_bytes);
            }
        }
        "turn/completed" => {
            let status = params.pointer("/turn/status").and_then(Value::as_str);
            let error = params
                .pointer("/turn/error/message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let mut state = turns.remove(thread_id).expect("turn exists");
            if let Some(sender) = state.completion.take() {
                let result = if status == Some("completed") && error.is_none() {
                    Ok(state.text.trim().to_owned())
                } else {
                    Err(AppServerError::Rpc(
                        error.unwrap_or_else(|| "Codex turn did not complete".into()),
                    ))
                };
                let _ = sender.send(result);
            }
        }
        _ => {}
    }
}

fn append_bounded(target: &mut String, value: &str, max_bytes: usize) {
    let remaining = max_bytes.saturating_sub(target.len());
    if remaining == 0 {
        return;
    }
    let mut boundary = remaining.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    target.push_str(&value[..boundary]);
}

async fn fail_all(inner: &Arc<Inner>) {
    for (_, sender) in inner.pending.lock().await.drain() {
        let _ = sender.send(Err(AppServerError::Closed));
    }
    for (_, mut state) in inner.turns.lock().await.drain() {
        if let Some(sender) = state.completion.take() {
            let _ = sender.send(Err(AppServerError::Closed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_kind_requires_a_status_token_or_reason_phrase() {
        assert_eq!(
            AppServerError::Rpc("unexpected status 502 Bad Gateway".into()).failure_kind(),
            "upstream_bad_gateway"
        );
        assert_eq!(
            AppServerError::Rpc("request id req-502-abc failed".into()).failure_kind(),
            "upstream_request_failed"
        );
    }

    #[test]
    fn codex_child_environment_excludes_qq_credentials() {
        let keys = child_environment(OsStr::new("codex-home"))
            .into_iter()
            .map(|(key, _)| key.to_string_lossy().to_ascii_uppercase())
            .collect::<Vec<_>>();
        assert!(!keys.iter().any(|key| key.starts_with("QQBOT_")));
        assert!(!keys.iter().any(|key| key == "OPENAI_API_KEY"));
        assert!(keys.iter().any(|key| key == "CODEX_HOME"));
    }

    #[test]
    fn server_requests_are_declined_with_method_specific_shapes() {
        assert_eq!(
            server_request_response(&json!(1), "item/commandExecution/requestApproval")
                .pointer("/result/decision"),
            Some(&json!("decline"))
        );
        assert_eq!(
            server_request_response(&json!(2), "mcpServer/elicitation/request")
                .pointer("/result/action"),
            Some(&json!("decline"))
        );
        assert_eq!(
            server_request_response(&json!(3), "item/tool/requestUserInput")
                .pointer("/result/answers"),
            Some(&json!({}))
        );
        assert_eq!(
            server_request_response(&json!(4), "unknown/method").pointer("/error/code"),
            Some(&json!(-32601))
        );
    }

    #[test]
    fn app_server_output_buffer_is_bounded_on_utf8_boundaries() {
        let mut output = String::from("a");
        append_bounded(&mut output, "你好世界", 8);
        assert!(output.len() <= 8);
        assert!(output.is_char_boundary(output.len()));
    }

    #[test]
    fn secure_arguments_disable_external_execution_surfaces() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            r#"
[mcp_servers.docs]
command = "docs-server"

"#,
        )
        .unwrap();
        let arguments = codex_arguments(false, home.path()).unwrap();
        for required in [
            "mcp_servers={}",
            "web_search=\"disabled\"",
            "mcp_servers.docs.enabled=false",
        ] {
            assert!(arguments.iter().any(|argument| argument == required));
        }
        for feature in EXTERNAL_TOOL_FEATURES {
            assert!(
                arguments
                    .windows(2)
                    .any(|pair| { pair[0] == "--disable" && pair[1] == *feature })
            );
        }
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--strict-config")
        );
        assert_eq!(
            codex_arguments(true, home.path()).unwrap(),
            ["app-server", "--stdio", "-c", "notify=[]"]
        );
    }

    #[test]
    fn unsafe_mcp_server_key_fails_closed() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("config.toml"),
            r#"
[mcp_servers."quoted.name"]
url = "https://example.invalid/mcp"
"#,
        )
        .unwrap();
        assert!(codex_arguments(false, home.path()).is_err());
    }

    #[test]
    fn external_tool_verification_fails_closed() {
        assert!(
            verify_effective_config(&json!({
                "config": {
                    "mcp_servers": {},
                    "web_search": "disabled",
                    "features": EXTERNAL_TOOL_FEATURES
                        .iter()
                        .map(|feature| ((*feature).to_owned(), Value::Bool(false)))
                        .collect::<serde_json::Map<String, Value>>()
                }
            }))
            .is_ok()
        );
        assert!(
            verify_effective_config(&json!({
                "config": {
                    "mcp_servers": {"disabled": {"enabled": false}},
                    "web_search": "disabled",
                    "features": EXTERNAL_TOOL_FEATURES
                        .iter()
                        .map(|feature| ((*feature).to_owned(), Value::Bool(false)))
                        .collect::<serde_json::Map<String, Value>>()
                }
            }))
            .is_ok()
        );
        assert!(
            verify_effective_config(&json!({
                "config": {
                    "mcp_servers": {"unsafe": {"enabled": true}},
                    "web_search": "disabled",
                    "features": EXTERNAL_TOOL_FEATURES
                        .iter()
                        .map(|feature| ((*feature).to_owned(), Value::Bool(false)))
                        .collect::<serde_json::Map<String, Value>>()
                }
            }))
            .is_err()
        );
        assert!(
            verify_hooks_disabled(&json!({
                "data": [{"hooks": [{"enabled": false}], "errors": []}]
            }))
            .is_ok()
        );
        assert!(
            verify_hooks_disabled(&json!({
                "data": [{"hooks": [{"enabled": true}], "errors": []}]
            }))
            .is_err()
        );
    }
}
