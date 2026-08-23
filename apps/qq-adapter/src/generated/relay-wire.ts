/* Generated from Rust relay-protocol. Do not edit; run `pnpm schema:generate`. */

/**
 * This interface was referenced by `QQToCodexRelayWireProtocol`'s JSON-Schema
 * via the `definition` "AdapterToCore".
 */
export type AdapterToCore =
  | {
      adapter: AdapterIdentity;
      protocol_version: string;
      schema_sha256: string;
      type: "hello";
    }
  | {
      protocol_version: string;
      type: "activate";
    }
  | {
      message: InboundMessage;
      protocol_version: string;
      type: "inbound";
    }
  | {
      outbox_id: string;
      protocol_version: string;
      type: "delivery_ack";
    }
  | {
      protocol_version: string;
      type: "shutdown";
    };
/**
 * This interface was referenced by `QQToCodexRelayWireProtocol`'s JSON-Schema
 * via the `definition` "ChatScope".
 */
export type ChatScope = "c2c" | "group";
/**
 * This interface was referenced by `QQToCodexRelayWireProtocol`'s JSON-Schema
 * via the `definition` "CoreToAdapter".
 */
export type CoreToAdapter =
  | {
      core_version: string;
      protocol_version: string;
      schema_sha256: string;
      type: "ready";
    }
  | {
      in_reply_to: string;
      outbox_id: string;
      protocol_version: string;
      target: ChatTarget;
      text: string;
      type: "reply";
    }
  | {
      code: string;
      in_reply_to?: string | null;
      message: string;
      outbox_id?: string | null;
      protocol_version: string;
      retryable: boolean;
      target?: ChatTarget | null;
      type: "error";
    }
  | {
      code: string;
      message: string;
      protocol_version: string;
      type: "fatal";
    };

export interface QQToCodexRelayWireProtocol {
  adapter_to_core: AdapterToCore;
  core_to_adapter: CoreToAdapter;
}
/**
 * This interface was referenced by `QQToCodexRelayWireProtocol`'s JSON-Schema
 * via the `definition` "AdapterIdentity".
 */
export interface AdapterIdentity {
  name: string;
  version: string;
}
/**
 * This interface was referenced by `QQToCodexRelayWireProtocol`'s JSON-Schema
 * via the `definition` "InboundMessage".
 */
export interface InboundMessage {
  content: string;
  message_id: string;
  platform_timestamp: string;
  received_at_ms: number;
  sender_id: string;
  sender_is_bot: boolean;
  target: ChatTarget;
}
/**
 * This interface was referenced by `QQToCodexRelayWireProtocol`'s JSON-Schema
 * via the `definition` "ChatTarget".
 */
export interface ChatTarget {
  scope: ChatScope;
  target_id: string;
}
