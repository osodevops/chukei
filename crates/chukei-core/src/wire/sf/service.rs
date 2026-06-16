//! chukei's own Snowflake session, used exclusively for actions the proxy
//! takes itself (today: `ALTER WAREHOUSE … SUSPEND` in enforce mode).
//! Client traffic never touches this session, and these credentials are
//! never used to answer client queries.

use std::sync::Mutex;

use crate::config::{Config, ServiceAccountConfig};
use crate::error::{Error, Result};

pub struct ServiceSession {
    client: reqwest::Client,
    base_url: String,
    account: String,
    credentials: ServiceAccountConfig,
    token: Mutex<Option<String>>,
}

impl ServiceSession {
    pub fn from_config(config: &Config) -> Result<Option<Self>> {
        if !config.service_account.is_configured() {
            return Ok(None);
        }
        let upstream =
            config.upstream.snowflake.as_ref().ok_or_else(|| {
                Error::Config("service_account requires upstream.snowflake".into())
            })?;
        Ok(Some(Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .map_err(|e| Error::Connectivity(e.to_string()))?,
            base_url: upstream.base_url().trim_end_matches('/').to_string(),
            account: upstream.account.clone(),
            credentials: config.service_account.clone(),
            token: Mutex::new(None),
        }))
    }

    async fn login(&self) -> Result<String> {
        let body = serde_json::json!({
            "data": {
                "ACCOUNT_NAME": self.account,
                "LOGIN_NAME": self.credentials.user,
                "PASSWORD": self.credentials.password,
                "CLIENT_APP_ID": "chukei",
                "CLIENT_APP_VERSION": env!("CARGO_PKG_VERSION"),
            }
        });
        let resp: serde_json::Value = self
            .client
            .post(format!(
                "{}/session/v1/login-request?requestId={}",
                self.base_url,
                uuid::Uuid::new_v4()
            ))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Connectivity(format!("service login failed: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Auth(format!("service login: bad response: {e}")))?;
        if resp.get("success").and_then(|s| s.as_bool()) != Some(true) {
            return Err(Error::Auth(format!(
                "service login rejected: {}",
                resp.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
            )));
        }
        let token = resp
            .pointer("/data/token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| Error::Auth("service login: no token in response".into()))?
            .to_string();
        // The session persists role state across requests on this token.
        if let Some(role) = &self.credentials.role {
            let _ = self
                .execute_with_token(&format!("USE ROLE {role}"), &token)
                .await;
        }
        *self.token.lock().unwrap() = Some(token.clone());
        Ok(token)
    }

    /// Execute one statement; logs in lazily and retries once on session
    /// expiry (Snowflake error 390112).
    pub async fn execute(&self, sql: &str) -> Result<serde_json::Value> {
        let token = {
            let current = self.token.lock().unwrap().clone();
            match current {
                Some(t) => t,
                None => self.login().await?,
            }
        };
        let resp = self.execute_with_token(sql, &token).await?;
        let expired = resp.get("code").and_then(|c| c.as_str()) == Some("390112");
        if !expired {
            return Ok(resp);
        }
        let fresh = self.login().await?;
        self.execute_with_token(sql, &fresh).await
    }

    async fn execute_with_token(&self, sql: &str, token: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "sqlText": sql, "sequenceId": 1 });
        self.client
            .post(format!(
                "{}/queries/v1/query-request?requestId={}",
                self.base_url,
                uuid::Uuid::new_v4()
            ))
            .header("Authorization", format!("Snowflake Token=\"{token}\""))
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Connectivity(format!("service statement failed: {e}")))?
            .json()
            .await
            .map_err(|e| Error::Connectivity(format!("service statement: bad response: {e}")))
    }
}
