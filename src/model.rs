use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const REGISTRY_SCHEMA_VERSION: u32 = 4;

fn schema_version() -> u32 {
    REGISTRY_SCHEMA_VERSION
}

fn live_interval() -> u16 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Registry {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub active_account_key: Option<String>,
    #[serde(default)]
    pub previous_active_account_key: Option<String>,
    #[serde(default)]
    pub active_account_activated_at_ms: Option<i64>,
    #[serde(default = "live_interval")]
    pub interval_seconds: u16,
    #[serde(default)]
    pub accounts: Vec<AccountRecord>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            active_account_key: None,
            previous_active_account_key: None,
            active_account_activated_at_ms: None,
            interval_seconds: live_interval(),
            accounts: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountRecord {
    pub account_key: String,
    pub chatgpt_account_id: String,
    pub chatgpt_user_id: String,
    pub email: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub account_name: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
    #[serde(default)]
    pub auth_mode: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub last_used_at: Option<i64>,
    #[serde(default)]
    pub last_usage: Option<RateLimitSnapshot>,
    #[serde(default)]
    pub last_usage_at: Option<i64>,
    #[serde(default)]
    pub last_local_rollout: Option<RolloutSignature>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl AccountRecord {
    pub fn display_name(&self) -> &str {
        if !self.alias.trim().is_empty() {
            self.alias.trim()
        } else if let Some(name) = self
            .account_name
            .as_deref()
            .filter(|v| !v.trim().is_empty())
        {
            name
        } else {
            &self.email
        }
    }

    pub fn is_api_key(&self) -> bool {
        self.auth_mode.as_deref() == Some("apikey")
    }

    pub fn display_plan(&self) -> &str {
        self.last_usage
            .as_ref()
            .and_then(|usage| usage.plan_type.as_deref())
            .or(self.plan.as_deref())
            .unwrap_or("unknown")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateLimitSnapshot {
    #[serde(default)]
    pub primary: Option<RateLimitWindow>,
    #[serde(default)]
    pub secondary: Option<RateLimitWindow>,
    #[serde(default)]
    pub credits: Option<CreditsSnapshot>,
    #[serde(default)]
    pub reset_credits: Option<i64>,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct RateLimitWindow {
    pub used_percent: f64,
    #[serde(default)]
    pub window_minutes: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreditsSnapshot {
    #[serde(default)]
    pub has_credits: bool,
    #[serde(default)]
    pub unlimited: bool,
    #[serde(default)]
    pub balance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RolloutSignature {
    pub path: String,
    pub event_timestamp_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trip_preserves_schema_four_fields_and_unknowns() {
        let input = r#"{
          "schema_version": 4,
          "active_account_key": "user-a::account-a",
          "previous_active_account_key": null,
          "active_account_activated_at_ms": 12,
          "interval_seconds": 60,
          "future_field": {"keep": true},
          "accounts": [{
            "account_key": "user-a::account-a",
            "chatgpt_account_id": "account-a",
            "chatgpt_user_id": "user-a",
            "email": "a@example.com",
            "alias": "work",
            "account_name": null,
            "plan": "pro",
            "auth_mode": "chatgpt",
            "created_at": 1,
            "last_used_at": null,
            "last_usage": null,
            "last_usage_at": null,
            "last_local_rollout": null,
            "account_future": 7
          }]
        }"#;
        let parsed: Registry = serde_json::from_str(input).unwrap();
        let output = serde_json::to_value(&parsed).unwrap();
        assert_eq!(output["future_field"]["keep"], true);
        assert_eq!(output["accounts"][0]["account_future"], 7);
        assert_eq!(parsed.schema_version, REGISTRY_SCHEMA_VERSION);
    }

    #[test]
    fn display_name_prefers_alias_then_account_name_then_email() {
        let mut record = sample_account();
        assert_eq!(record.display_name(), "work");
        record.alias.clear();
        record.account_name = Some("Workspace".into());
        assert_eq!(record.display_name(), "Workspace");
        record.account_name = None;
        assert_eq!(record.display_name(), "a@example.com");
    }

    fn sample_account() -> AccountRecord {
        AccountRecord {
            account_key: "user-a::account-a".into(),
            chatgpt_account_id: "account-a".into(),
            chatgpt_user_id: "user-a".into(),
            email: "a@example.com".into(),
            alias: "work".into(),
            account_name: None,
            plan: Some("pro".into()),
            auth_mode: Some("chatgpt".into()),
            created_at: 1,
            last_used_at: None,
            last_usage: None,
            last_usage_at: None,
            last_local_rollout: None,
            extra: BTreeMap::new(),
        }
    }
}
