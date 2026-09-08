use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;

use crate::model::AccountRecord;

#[derive(Debug, Clone, PartialEq)]
pub enum AuthKind {
    ChatGpt,
    ApiKey,
}

#[derive(Debug, Clone)]
pub struct AuthInfo {
    pub kind: AuthKind,
    pub email: Option<String>,
    pub chatgpt_account_id: Option<String>,
    pub chatgpt_user_id: Option<String>,
    pub record_key: Option<String>,
    pub access_token: Option<String>,
    pub plan: Option<String>,
}

impl AuthInfo {
    pub fn into_account(self, alias: String, created_at: i64) -> Result<AccountRecord> {
        if self.kind == AuthKind::ApiKey {
            bail!("API-key accounts can be displayed but cannot be imported by the v1 login flow");
        }
        Ok(AccountRecord {
            account_key: self
                .record_key
                .context("auth token is missing a stable account key")?,
            chatgpt_account_id: self
                .chatgpt_account_id
                .context("auth token is missing a ChatGPT account id")?,
            chatgpt_user_id: self
                .chatgpt_user_id
                .context("auth token is missing a ChatGPT user id")?,
            email: self
                .email
                .context("auth token is missing an email address")?,
            alias,
            account_name: None,
            plan: self.plan,
            auth_mode: Some("chatgpt".into()),
            created_at,
            last_used_at: None,
            last_usage: None,
            last_usage_at: None,
            last_local_rollout: None,
            extra: BTreeMap::new(),
        })
    }
}

pub fn parse_auth_file(path: &Path) -> Result<AuthInfo> {
    let data = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    parse_auth_bytes(&data)
}

pub fn parse_auth_bytes(data: &[u8]) -> Result<AuthInfo> {
    let root: Value = serde_json::from_slice(data).context("parse auth.json")?;
    if root
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(AuthInfo {
            kind: AuthKind::ApiKey,
            email: None,
            chatgpt_account_id: None,
            chatgpt_user_id: None,
            record_key: None,
            access_token: None,
            plan: None,
        });
    }

    let tokens = root
        .get("tokens")
        .and_then(Value::as_object)
        .context("auth.json is missing its tokens object")?;
    let id_token = tokens
        .get("id_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .context("auth.json is missing its ID token")?;
    let access_token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let token_account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());

    let payload = decode_jwt_payload(id_token)?;
    let claims: Value = serde_json::from_slice(&payload).context("parse ID-token claims")?;
    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase());
    let auth_claims = claims
        .get("https://api.openai.com/auth")
        .and_then(Value::as_object);

    let claim_account_id = auth_claims
        .and_then(|object| object.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let organization_account_id = auth_claims.and_then(default_organization_id);
    let chatgpt_account_id = token_account_id
        .or(claim_account_id)
        .or(organization_account_id)
        .map(str::to_owned);
    let chatgpt_user_id = auth_claims
        .and_then(|object| {
            object
                .get("chatgpt_user_id")
                .or_else(|| object.get("user_id"))
        })
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let plan = auth_claims
        .and_then(|object| object.get("chatgpt_plan_type"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(normalize_plan);
    let record_key = match (&chatgpt_user_id, &chatgpt_account_id) {
        (Some(user), Some(account)) => Some(format!("{user}::{account}")),
        _ => None,
    };

    Ok(AuthInfo {
        kind: AuthKind::ChatGpt,
        email,
        chatgpt_account_id,
        chatgpt_user_id,
        record_key,
        access_token,
        plan,
    })
}

fn decode_jwt_payload(jwt: &str) -> Result<Vec<u8>> {
    let mut parts = jwt.split('.');
    let _header = parts.next();
    let payload = parts.next().context("ID token is not a JWT")?;
    let _signature = parts.next().context("ID token is not a JWT")?;
    URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| anyhow!("decode ID-token payload: {error}"))
}

fn default_organization_id(object: &serde_json::Map<String, Value>) -> Option<&str> {
    let organizations = object.get("organizations")?.as_array()?;
    organizations
        .iter()
        .filter_map(Value::as_object)
        .find(|organization| {
            organization
                .get("is_default")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && organization.get("id").and_then(Value::as_str).is_some()
        })
        .or_else(|| organizations.iter().filter_map(Value::as_object).next())
        .and_then(|organization| organization.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

pub fn normalize_plan(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "team" | "self_serve_business_usage_based" => "business".into(),
        "business" | "enterprise_cbp_usage_based" | "enterprise" | "hc" => "enterprise".into(),
        "education" | "edu" => "edu".into(),
        "pro_lite" | "prolite" => "prolite".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::json;

    fn auth_with_claims(claims: Value, token_account_id: Option<&str>) -> Vec<u8> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        serde_json::to_vec(&json!({
            "tokens": {
                "id_token": format!("{header}.{payload}.sig"),
                "access_token": "access",
                "account_id": token_account_id
            }
        }))
        .unwrap()
    }

    #[test]
    fn parses_chatgpt_auth_and_stable_key() {
        let data = auth_with_claims(
            json!({
                "email": "Person@Example.com",
                "https://api.openai.com/auth": {
                    "chatgpt_account_id": "account-a",
                    "chatgpt_user_id": "user-a",
                    "chatgpt_plan_type": "team"
                }
            }),
            None,
        );
        let info = parse_auth_bytes(&data).unwrap();
        assert_eq!(info.email.as_deref(), Some("person@example.com"));
        assert_eq!(info.record_key.as_deref(), Some("user-a::account-a"));
        assert_eq!(info.plan.as_deref(), Some("business"));
    }

    #[test]
    fn token_account_id_wins_over_claim() {
        let data = auth_with_claims(
            json!({"https://api.openai.com/auth": {
                "chatgpt_account_id": "claim-account",
                "chatgpt_user_id": "user-a"
            }}),
            Some("token-account"),
        );
        let info = parse_auth_bytes(&data).unwrap();
        assert_eq!(info.chatgpt_account_id.as_deref(), Some("token-account"));
    }

    #[test]
    fn falls_back_to_default_organization() {
        let data = auth_with_claims(
            json!({"https://api.openai.com/auth": {
                "chatgpt_user_id": "user-a",
                "organizations": [
                    {"id": "first", "is_default": false},
                    {"id": "default", "is_default": true}
                ]
            }}),
            None,
        );
        let info = parse_auth_bytes(&data).unwrap();
        assert_eq!(info.chatgpt_account_id.as_deref(), Some("default"));
    }

    #[test]
    fn identifies_api_key_auth() {
        let info = parse_auth_bytes(br#"{"OPENAI_API_KEY":"sk-test"}"#).unwrap();
        assert_eq!(info.kind, AuthKind::ApiKey);
    }
}
