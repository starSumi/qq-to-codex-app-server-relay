import {
  QQBot,
  type QQBotInboundMessage,
  type ReplyTarget,
} from "@tencent-connect/qqbot-nodejs";
import { chunkText } from "@tencent-connect/qqbot-nodejs/protocol";

import { loadConfig } from "./config.js";
import { CoreBridge } from "./core-bridge.js";
import {
  resolveQQCredentials,
  type QQCredentialStatus,
} from "./credentials.js";
import type { CoreToAdapter, InboundMessage } from "./generated/relay-wire.js";
import {
  createRedactingLogger,
  errorMessage,
  redactLogMessage,
} from "./logger.js";
import { normalizeMessage } from "./normalize.js";

function reportCredentialStatus(status: QQCredentialStatus): void {
  const messages: Record<QQCredentialStatus, string> = {
    environment_loaded: "QQ Bot credentials loaded from environment.",
    file_loaded: "QQ Bot credentials loaded from local credential file.",
    qr_required: "Scan the QR code with mobile QQ to bind the bot.",
    qr_failed: "QQ Bot QR binding or credential persistence failed.",
    qr_succeeded: "QQ Bot QR binding succeeded; credentials saved locally.",
  };
  console.info(messages[status]);
}

async function sendOutput(
  bot: QQBot,
  bridge: CoreBridge,
  output: CoreToAdapter,
): Promise<void> {
  if (output.type !== "reply" && output.type !== "error") return;
  if (!output.target || !output.in_reply_to) {
    console.warn(`relay output ${output.type} has no QQ delivery target`);
    return;
  }
  const target: ReplyTarget = {
    scope: output.target.scope,
    targetId: output.target.target_id,
    msgId: output.in_reply_to,
  };
  const text = output.type === "reply" ? output.text : output.message;
  for (const part of chunkText(text, 5_000)) {
    await bot.sendText(target, part);
  }
  if (output.outbox_id) await bridge.acknowledge(output.outbox_id);
}

async function main(): Promise<void> {
  const config = loadConfig();
  const credentials = await resolveQQCredentials({
    configured: config.qqCredentials,
    credentialFile: config.qqCredentialsFile,
    workspace: config.codexWorkspace,
    onStatus: reportCredentialStatus,
  });
  fatalSecrets = [credentials.appId, credentials.appSecret];
  const logger = createRedactingLogger(fatalSecrets, config.qqLogLevel);
  const bridge = new CoreBridge(
    config.coreBin,
    config.coreCwd,
    config.coreEnvironment,
    logger,
  );
  const bot = new QQBot({
    appId: credentials.appId,
    appSecret: credentials.appSecret,
    accountId: "qq-to-codex-app-server-relay",
    logger,
    tokenPrefetch: "sync",
    transport: "websocket",
  });
  const buffered: InboundMessage[] = [];
  let active = false;
  let stopping = false;

  const shutdown = async (): Promise<void> => {
    if (stopping) return;
    stopping = true;
    logger.info("relay shutting down");
    bot.stop();
    await bridge.stop();
  };

  bridge.onFailure((error) => {
    logger.error(`relay core failure: ${errorMessage(error)}`);
    process.exitCode = 1;
    void shutdown();
  });
  bridge.onOutput(async (output) => {
    if (output.type === "fatal") {
      logger.error(`relay core fatal: ${output.code}`);
      await shutdown();
      return;
    }
    await sendOutput(bot, bridge, output);
  });

  bot.on("error", (error) =>
    logger.error(`QQ gateway error: ${errorMessage(error)}`),
  );
  bot.on("message", (_context, message: QQBotInboundMessage) => {
    const normalized = normalizeMessage(message);
    if (!normalized) return;
    if (!active) {
      if (buffered.length >= 32) {
        logger.warn(
          "dropping QQ message while relay activation buffer is full",
        );
        return;
      }
      buffered.push(normalized);
      return;
    }
    void bridge
      .sendInbound(normalized)
      .catch((error: unknown) =>
        logger.error(`cannot forward QQ message: ${errorMessage(error)}`),
      );
  });

  process.once("SIGINT", () => void shutdown());
  process.once("SIGTERM", () => void shutdown());

  try {
    logger.info("starting relay core and Codex app-server");
    await bridge.start();
    logger.info("Codex app-server ready");
    let markGatewayReady: (() => void) | undefined;
    let rejectGatewayReady: ((error: Error) => void) | undefined;
    const gatewayReady = new Promise<void>((resolve, reject) => {
      markGatewayReady = resolve;
      rejectGatewayReady = reject;
    });
    bot.on("ready", () => {
      logger.info("QQ gateway connected");
      markGatewayReady?.();
    });
    const gatewayRun = bot.start();
    void gatewayRun.catch((error: unknown) =>
      rejectGatewayReady?.(new Error(errorMessage(error))),
    );
    let gatewayReadyTimer: NodeJS.Timeout | undefined;
    try {
      await Promise.race([
        gatewayReady,
        new Promise<never>((_resolve, reject) => {
          gatewayReadyTimer = setTimeout(
            () => reject(new Error("QQ gateway readiness timed out")),
            config.gatewayReadyTimeoutMs,
          );
        }),
      ]);
    } finally {
      if (gatewayReadyTimer) clearTimeout(gatewayReadyTimer);
    }
    await bridge.activate();
    active = true;
    logger.info("relay active; waiting for QQ messages");
    for (const message of buffered.splice(0)) await bridge.sendInbound(message);
    await gatewayRun;
  } finally {
    await shutdown();
  }
}

let fatalSecrets = [
  process.env["QQBOT_APP_ID"],
  process.env["QQBOT_APP_SECRET"],
].filter((value): value is string => Boolean(value));

main().catch((error: unknown) => {
  console.error(
    `relay fatal: ${redactLogMessage(errorMessage(error), fatalSecrets)}; inspect redacted local logs`,
  );
  process.exitCode = 1;
});
