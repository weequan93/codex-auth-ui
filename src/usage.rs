use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result};
use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;
use thiserror::Error;

use crate::{
    auth::{normalize_plan, parse_auth_file, AuthKind},
    model::{CreditsSnapshot, RateLimitSnapshot, RateLimitWindow},
};

pub const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("Needs re-login")]
    NeedsRelogin,
    #[error("Network error: {0}")]
    Network(String),
    #[error("Quota service returned HTTP {0}")]
    Http(u16),
    #[error("No quota data was returned")]
    NoData,
    #[error("{0}")]
    Other(String),
}

pub fn build_client() -> Result<Client> {
    Client::builder()
        .user_agent(format!("codex-account-hub/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .context("construct the shared quota client")
}

pub fn fetch_for_auth(
    client: &Client,
    auth_path: &std::path::Path,
) -> std::result::Result<RateLimitSnapshot, RefreshError> {
    let info =
        parse_auth_file(auth_path).map_err(|error| RefreshError::Other(error.to_string()))?;
    if info.kind != AuthKind::ChatGpt {
        return Err(RefreshError::Other(
            "Quota is unavailable for API-key accounts".into(),
        ));
    }
    let token = info
        .access_token
        .ok_or_else(|| RefreshError::Other("Stored credentials have no access token".into()))?;
    let account_id = info
        .chatgpt_account_id
        .ok_or_else(|| RefreshError::Other("Stored credentials have no account id".into()))?;

    let response = client
        .get(USAGE_ENDPOINT)
        .bearer_auth(token)
        .header("ChatGPT-Account-Id", account_id)
        .header("Accept", "application/json")
        .send()
        .map_err(|error| RefreshError::Network(network_message(&error)))?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(RefreshError::NeedsRelogin);
    }
    if !status.is_success() {
        return Err(RefreshError::Http(status.as_u16()));
    }
    let body = response
        .bytes()
        .map_err(|error| RefreshError::Network(network_message(&error)))?;
    parse_usage_response(&body)
        .map_err(|error| RefreshError::Other(error.to_string()))?
        .ok_or(RefreshError::NoData)
}

fn network_message(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".into()
    } else if error.is_connect() {
        "could not connect".into()
    } else {
        error.to_string()
    }
}

pub fn parse_usage_response(data: &[u8]) -> Result<Option<RateLimitSnapshot>> {
    let root: Value = serde_json::from_slice(data).context("parse quota response")?;
    let Some(object) = root.as_object() else {
        return Ok(None);
    };
    let plan_type = object
        .get("plan_type")
        .and_then(Value::as_str)
        .map(normalize_plan);
    let credits = object.get("credits").and_then(parse_credits);
    let reset_credits = object
        .get("rate_limit_reset_credits")
        .and_then(Value::as_object)
        .and_then(|value| value.get("available_count"))
        .and_then(Value::as_i64);
    let rate_limit = object.get("rate_limit").and_then(Value::as_object);
    let primary = rate_limit
        .and_then(|value| value.get("primary_window").or_else(|| value.get("primary")))
        .and_then(parse_window);
    let secondary = rate_limit
        .and_then(|value| {
            value
                .get("secondary_window")
                .or_else(|| value.get("secondary"))
        })
        .and_then(parse_window);

    if primary.is_none() && secondary.is_none() && reset_credits.is_none() {
        return Ok(None);
    }
    Ok(Some(RateLimitSnapshot {
        primary,
        secondary,
        credits,
        reset_credits,
        plan_type,
        extra: BTreeMap::new(),
    }))
}

fn parse_window(value: &Value) -> Option<RateLimitWindow> {
    let object = value.as_object()?;
    let used_percent = object.get("used_percent")?.as_f64()?;
    let window_minutes = object
        .get("window_minutes")
        .and_then(Value::as_i64)
        .or_else(|| {
            object
                .get("limit_window_seconds")
                .and_then(Value::as_i64)
                .filter(|seconds| *seconds > 0)
                .map(|seconds| seconds.saturating_add(59) / 60)
        });
    let resets_at = object
        .get("resets_at")
        .or_else(|| object.get("reset_at"))
        .and_then(Value::as_i64);
    Some(RateLimitWindow {
        used_percent,
        window_minutes,
        resets_at,
    })
}

fn parse_credits(value: &Value) -> Option<CreditsSnapshot> {
    let object = value.as_object()?;
    Some(CreditsSnapshot {
        has_credits: object
            .get("has_credits")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        unlimited: object
            .get("unlimited")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        balance: object
            .get("balance")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_windows_credits_and_plan() {
        let payload = br#"{
          "plan_type": "team",
          "rate_limit": {
            "primary_window": {"used_percent": 12.5, "limit_window_seconds": 18000, "reset_at": 2000},
            "secondary_window": {"used_percent": 44, "limit_window_seconds": 604800, "reset_at": 3000}
          },
          "credits": {"has_credits": true, "unlimited": false, "balance": "10.50"},
          "rate_limit_reset_credits": {"available_count": 2}
        }"#;
        let usage = parse_usage_response(payload).unwrap().unwrap();
        assert_eq!(usage.plan_type.as_deref(), Some("business"));
        assert_eq!(usage.primary.unwrap().window_minutes, Some(300));
        assert_eq!(usage.secondary.unwrap().window_minutes, Some(10080));
        assert_eq!(usage.credits.unwrap().balance.as_deref(), Some("10.50"));
        assert_eq!(usage.reset_credits, Some(2));
    }

    #[test]
    fn empty_payload_has_no_displayable_snapshot() {
        assert!(parse_usage_response(br#"{"plan_type":"plus"}"#)
            .unwrap()
            .is_none());
    }

    #[test]
    fn extreme_window_duration_does_not_overflow() {
        let value = serde_json::json!({"used_percent": 5, "limit_window_seconds": i64::MAX});
        assert!(parse_window(&value).unwrap().window_minutes.unwrap() > 0);
    }

    #[test]
    fn accepts_persisted_window_field_names() {
        let payload =
            br#"{"rate_limit":{"primary":{"used_percent":5,"window_minutes":300,"resets_at":9}}}"#;
        let usage = parse_usage_response(payload).unwrap().unwrap();
        assert_eq!(usage.primary.unwrap().resets_at, Some(9));
    }
}
