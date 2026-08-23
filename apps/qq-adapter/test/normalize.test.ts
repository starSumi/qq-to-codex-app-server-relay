import type { QQBotInboundMessage } from "@tencent-connect/qqbot-nodejs";
import { describe, expect, it } from "vitest";

import { normalizeMessage } from "../src/normalize.js";

function message(
  overrides: Partial<QQBotInboundMessage> = {},
): QQBotInboundMessage {
  return {
    rawEventType: "C2C_MESSAGE_CREATE",
    kind: "c2c",
    senderId: "sender",
    content: " hello ",
    messageId: "message",
    timestamp: "2026-08-22T00:00:00Z",
    raw: {} as QQBotInboundMessage["raw"],
    replyTarget: { scope: "c2c", targetId: "sender", msgId: "message" },
    ...overrides,
  };
}

describe("normalizeMessage", () => {
  it("normalizes the supported QQ envelope without copying raw platform data", () => {
    expect(normalizeMessage(message())).toMatchObject({
      message_id: "message",
      sender_id: "sender",
      content: "hello",
      target: { scope: "c2c", target_id: "sender" },
    });
  });

  it("drops empty and unsupported messages", () => {
    expect(normalizeMessage(message({ content: "  " }))).toBeUndefined();
    expect(normalizeMessage(message({ kind: "guild" }))).toBeUndefined();
  });
});
