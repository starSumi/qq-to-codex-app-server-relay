import type { QQBotInboundMessage } from "@tencent-connect/qqbot-nodejs";

import type { InboundMessage } from "./generated/relay-wire.js";

export function normalizeMessage(
  message: QQBotInboundMessage,
): InboundMessage | undefined {
  if (message.kind !== "c2c" && message.kind !== "group") return undefined;
  const content = message.content.trim();
  if (!content) return undefined;
  return {
    message_id: message.messageId,
    sender_id: message.senderId,
    sender_is_bot: message.senderIsBot === true,
    target: {
      scope: message.replyTarget.scope,
      target_id: message.replyTarget.targetId,
    },
    content,
    platform_timestamp: message.timestamp,
    received_at_ms: Date.now(),
  };
}
