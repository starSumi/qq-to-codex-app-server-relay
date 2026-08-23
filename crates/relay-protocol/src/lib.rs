use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROTOCOL_VERSION: &str = "1";
pub const SCHEMA_ID: &str =
    "https://github.com/starSumi/qq-to-codex-app-server-relay/schemas/relay-wire.schema.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatScope {
    C2c,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatTarget {
    pub scope: ChatScope,
    pub target_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdapterIdentity {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InboundMessage {
    pub message_id: String,
    pub sender_id: String,
    pub sender_is_bot: bool,
    pub target: ChatTarget,
    pub content: String,
    pub platform_timestamp: String,
    pub received_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdapterToCore {
    Hello {
        protocol_version: String,
        schema_sha256: String,
        adapter: AdapterIdentity,
    },
    Activate {
        protocol_version: String,
    },
    Inbound {
        protocol_version: String,
        message: InboundMessage,
    },
    DeliveryAck {
        protocol_version: String,
        outbox_id: String,
    },
    Shutdown {
        protocol_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreToAdapter {
    Ready {
        protocol_version: String,
        schema_sha256: String,
        core_version: String,
    },
    Reply {
        protocol_version: String,
        outbox_id: String,
        in_reply_to: String,
        target: ChatTarget,
        text: String,
    },
    Error {
        protocol_version: String,
        outbox_id: Option<String>,
        in_reply_to: Option<String>,
        target: Option<ChatTarget>,
        code: String,
        message: String,
        retryable: bool,
    },
    Fatal {
        protocol_version: String,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelayWireSchema {
    pub adapter_to_core: AdapterToCore,
    pub core_to_adapter: CoreToAdapter,
}

pub fn schema_json() -> anyhow::Result<String> {
    let schema = schema_for!(RelayWireSchema);
    let mut value = serde_json::to_value(schema)?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("generated schema root is not an object"))?;
    root.insert("$id".into(), serde_json::Value::String(SCHEMA_ID.into()));
    root.insert(
        "title".into(),
        serde_json::Value::String("QQ to Codex relay wire protocol".into()),
    );
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

#[must_use]
pub fn schema_sha256(schema: &str) -> String {
    format!("{:x}", Sha256::digest(schema.as_bytes()))
}

pub fn current_schema_sha256() -> anyhow::Result<String> {
    Ok(schema_sha256(&schema_json()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_messages_reject_unknown_fields() {
        let input = r#"{"type":"shutdown","protocol_version":"1","unexpected":true}"#;
        assert!(serde_json::from_str::<AdapterToCore>(input).is_err());
    }

    #[test]
    fn generated_schema_is_stable_within_a_build() {
        let first = schema_json().expect("schema");
        let second = schema_json().expect("schema");
        assert_eq!(first, second);
        assert_eq!(schema_sha256(&first).len(), 64);
    }
}
