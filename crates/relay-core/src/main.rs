mod app_server;
mod config;
mod engine;
mod storage;

use std::{collections::HashSet, io::Write, sync::Arc, time::Duration};

use anyhow::Context;
use relay_protocol::{AdapterToCore, CoreToAdapter, InboundMessage, PROTOCOL_VERSION};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Semaphore, mpsc},
    task::JoinSet,
    time,
};
use tracing_subscriber::EnvFilter;

use crate::{
    app_server::AppServerClient,
    config::{AuthorizationMode, Config},
    engine::Engine,
    storage::Store,
};

fn init_logging() {
    let filter = std::env::var("RELAY_LOG")
        .ok()
        .and_then(|value| EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .without_time()
        .init();
}

fn print_protocol_message(message: &CoreToAdapter) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, message)?;
    lock.write_all(b"\n")?;
    lock.flush()?;
    Ok(())
}

fn fatal(code: &str, message: &str) -> CoreToAdapter {
    CoreToAdapter::Fatal {
        protocol_version: PROTOCOL_VERSION.into(),
        code: code.into(),
        message: message.into(),
    }
}

#[tokio::main]
async fn main() {
    init_logging();
    if let Err(error) = run().await {
        tracing::error!(error = %error, "relay core stopped");
        let _ = print_protocol_message(&fatal(
            "core_startup_failed",
            "Relay core failed. See the local redacted logs for details.",
        ));
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let config = Arc::new(Config::from_env().context("invalid relay configuration")?);
    if config.auth_mode.is_dangerous() {
        tracing::warn!(
            "DANGEROUS AUTH MODE ENABLED: every non-bot QQ sender can trigger Codex; keep this deployment private"
        );
    }
    tracing::info!(
        sandbox = ?config.sandbox,
        network_access = config.network_access,
        external_tools = config.allow_external_tools,
        "Codex execution profile configured"
    );
    let store = Arc::new(Store::open(&config.database, &config.workspace)?);
    let recovered = store.recover_interrupted()?;
    if recovered > 0 {
        tracing::warn!(recovered, "marked interrupted inbox rows as failed");
    }
    let purged = store.purge_expired(config.retention)?;
    if purged != storage::PurgeCounts::default() {
        tracing::info!(
            outbox = purged.outbox,
            inbox = purged.inbox,
            threads = purged.threads,
            "purged expired relay metadata"
        );
    }
    let schema_sha256 = relay_protocol::current_schema_sha256()?;

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let hello_line = time::timeout(Duration::from_secs(15), lines.next_line())
        .await
        .context("adapter handshake timed out")??
        .context("adapter closed before handshake")?;
    let hello = serde_json::from_str::<AdapterToCore>(&hello_line)
        .context("adapter handshake is not valid protocol JSON")?;
    match hello {
        AdapterToCore::Hello {
            protocol_version,
            schema_sha256: adapter_schema,
            adapter,
        } if protocol_version == PROTOCOL_VERSION && adapter_schema == schema_sha256 => {
            tracing::info!(adapter = %adapter.name, version = %adapter.version, "adapter handshake accepted");
        }
        _ => anyhow::bail!("adapter protocol or schema mismatch"),
    }

    let app_server = AppServerClient::start(&config).await?;
    let result = serve(
        Arc::clone(&config),
        Arc::clone(&store),
        app_server.clone(),
        schema_sha256,
        lines,
    )
    .await;
    app_server.stop().await;
    result
}

async fn serve(
    config: Arc<Config>,
    store: Arc<Store>,
    app_server: AppServerClient,
    schema_sha256: String,
    mut lines: tokio::io::Lines<BufReader<tokio::io::Stdin>>,
) -> anyhow::Result<()> {
    print_protocol_message(&CoreToAdapter::Ready {
        protocol_version: PROTOCOL_VERSION.into(),
        schema_sha256,
        core_version: env!("CARGO_PKG_VERSION").into(),
    })?;

    let (output_tx, mut output_rx) = mpsc::channel::<CoreToAdapter>(config.max_queue);
    let output_task = tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(message) = output_rx.recv().await {
            let mut bytes = serde_json::to_vec(&message)?;
            bytes.push(b'\n');
            stdout.write_all(&bytes).await?;
            stdout.flush().await?;
        }
        Ok::<(), anyhow::Error>(())
    });

    let engine = Arc::new(Engine::new(
        Arc::clone(&config),
        Arc::clone(&store),
        app_server.clone(),
    ));
    let (inbound_tx, mut inbound_rx) = mpsc::channel(config.max_queue);
    let dispatcher_output = output_tx.clone();
    let concurrency = Arc::new(Semaphore::new(config.max_concurrent_turns));
    let mut dispatcher = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        while let Some(message) = inbound_rx.recv().await {
            let permit = Arc::clone(&concurrency).acquire_owned().await?;
            let engine = Arc::clone(&engine);
            let output = dispatcher_output.clone();
            tasks.spawn(async move {
                let _permit = permit;
                match engine.process(message).await {
                    Ok(Some(response)) => {
                        output.send(response).await.context("output channel closed")
                    }
                    Ok(None) => Ok(()),
                    Err(error) => {
                        tracing::error!("inbound message failed before durable completion");
                        output.send(fatal(
                            "core_processing_failed",
                            "Relay state could not be completed safely. The relay will restart.",
                        )).await.context("output channel closed")?;
                        Err(error)
                    }
                }
            });
            while tasks.len() >= 256 {
                if let Some(result) = tasks.join_next().await {
                    result??;
                }
            }
        }
        while let Some(result) = tasks.join_next().await {
            result??;
        }
        Ok::<(), anyhow::Error>(())
    });

    let session_result = adapter_loop(
        &mut lines,
        &inbound_tx,
        &output_tx,
        &store,
        config.auth_mode,
        &config.allow_from,
    )
    .await;

    drop(inbound_tx);
    let dispatcher_result = match time::timeout(Duration::from_secs(30), &mut dispatcher).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(error).context("relay dispatcher task failed"),
        Err(_) => {
            dispatcher.abort();
            let _ = dispatcher.await;
            Err(anyhow::anyhow!("timed out draining relay queue"))
        }
    };
    drop(output_tx);
    let output_result = match output_task.await {
        Ok(result) => result,
        Err(error) => Err(error).context("relay output task failed"),
    };
    session_result?;
    dispatcher_result?;
    output_result
}

async fn adapter_loop(
    lines: &mut tokio::io::Lines<BufReader<tokio::io::Stdin>>,
    inbound_tx: &mpsc::Sender<InboundMessage>,
    output_tx: &mpsc::Sender<CoreToAdapter>,
    store: &Store,
    auth_mode: AuthorizationMode,
    allow_from: &HashSet<String>,
) -> anyhow::Result<()> {
    let mut active = false;
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break };
                let message = serde_json::from_str::<AdapterToCore>(&line)
                    .context("adapter sent invalid protocol JSON")?;
                match message {
                    AdapterToCore::Inbound { protocol_version, message } => {
                        if protocol_version != PROTOCOL_VERSION {
                            anyhow::bail!("adapter protocol version changed after handshake");
                        }
                        if !active {
                            anyhow::bail!("adapter sent inbound traffic before activation");
                        }
                        if !sender_authorized(&message, auth_mode, allow_from) {
                            continue;
                        }
                        if let Err(error) = inbound_tx.try_send(message) {
                            let message = error.into_inner();
                            output_tx.send(CoreToAdapter::Error {
                                protocol_version: PROTOCOL_VERSION.into(),
                                outbox_id: None,
                                in_reply_to: Some(message.message_id),
                                target: Some(message.target),
                                code: "queue_full".into(),
                                message: "The relay is busy. Please try again later.".into(),
                                retryable: true,
                            }).await?;
                        }
                    }
                    AdapterToCore::DeliveryAck { protocol_version, outbox_id } => {
                        if protocol_version != PROTOCOL_VERSION || outbox_id.len() > 64 {
                            anyhow::bail!("invalid delivery acknowledgement");
                        }
                        if !store.acknowledge(&outbox_id)? {
                            tracing::warn!("ignored unknown or duplicate delivery acknowledgement");
                        }
                    }
                    AdapterToCore::Shutdown { protocol_version } => {
                        if protocol_version != PROTOCOL_VERSION {
                            anyhow::bail!("invalid shutdown protocol version");
                        }
                        break;
                    }
                    AdapterToCore::Activate { protocol_version } => {
                        if protocol_version != PROTOCOL_VERSION || active {
                            anyhow::bail!("invalid or duplicate adapter activation");
                        }
                        active = true;
                        for pending in store.pending_outbox()? {
                            output_tx.send(pending).await?;
                        }
                    }
                    AdapterToCore::Hello { .. } => anyhow::bail!("duplicate adapter handshake"),
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                break;
            }
        }
    }
    Ok(())
}

fn sender_authorized(
    message: &InboundMessage,
    auth_mode: AuthorizationMode,
    allow_from: &HashSet<String>,
) -> bool {
    auth_mode.authorizes(message.sender_is_bot, &message.sender_id, allow_from)
}

#[cfg(test)]
mod tests {
    use super::sender_authorized;
    use crate::config::AuthorizationMode;
    use relay_protocol::{ChatScope, ChatTarget, InboundMessage};
    use std::collections::HashSet;

    fn message(sender_id: &str, sender_is_bot: bool) -> InboundMessage {
        InboundMessage {
            message_id: "message-1".to_owned(),
            sender_id: sender_id.to_owned(),
            sender_is_bot,
            target: ChatTarget {
                scope: ChatScope::C2c,
                target_id: sender_id.to_owned(),
            },
            content: "hello".to_owned(),
            platform_timestamp: "1".to_owned(),
            received_at_ms: 1,
        }
    }

    #[test]
    fn admission_rejects_unauthorized_and_bot_senders() {
        let allow_from = HashSet::from(["authorized".to_owned()]);

        assert!(sender_authorized(
            &message("authorized", false),
            AuthorizationMode::Safe,
            &allow_from
        ));
        assert!(!sender_authorized(
            &message("unauthorized", false),
            AuthorizationMode::Safe,
            &allow_from
        ));
        assert!(!sender_authorized(
            &message("authorized", true),
            AuthorizationMode::Safe,
            &allow_from
        ));
    }

    #[test]
    fn dangerous_mode_allows_non_bot_senders_without_allowlist() {
        let allow_from = HashSet::new();
        assert!(sender_authorized(
            &message("unknown", false),
            AuthorizationMode::Dangerous,
            &allow_from
        ));
        assert!(!sender_authorized(
            &message("unknown", true),
            AuthorizationMode::Dangerous,
            &allow_from
        ));
    }
}
