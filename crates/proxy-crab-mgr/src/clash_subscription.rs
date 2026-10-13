use std::{
    net::Ipv4Addr,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{RawQuery, State},
    http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, REFERRER_POLICY},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::{
    ProxyCrabManager,
    dto::{ManagerError, ManagerResult},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClashSubscriptionState {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnableClashSubscriptionRequest {
    pub server: String,
}

#[derive(Default)]
pub(crate) struct ClashSubscription {
    state: Mutex<ClashSubscriptionState>,
}

impl ClashSubscription {
    pub fn state(&self) -> ClashSubscriptionState {
        self.state
            .lock()
            .expect("Clash subscription lock poisoned")
            .clone()
    }

    pub fn enable(&self, server: &str) -> ManagerResult<ClashSubscriptionState> {
        let ip: Ipv4Addr = server
            .parse()
            .map_err(|_| ManagerError::bad_request("server must be a non-loopback IPv4 address"))?;
        if ip.octets()[0] == 0 || ip.is_loopback() || ip.is_multicast() || ip.is_broadcast() {
            return Err(ManagerError::bad_request(
                "server must be a non-loopback unicast IPv4 address",
            ));
        }
        let mut state = self.state.lock().expect("Clash subscription lock poisoned");
        if state.token.is_none() {
            let mut bytes = [0; 32];
            getrandom::fill(&mut bytes).map_err(|error| {
                ManagerError::internal(format!("generate Clash subscription token: {error}"))
            })?;
            state.token = Some(format!("pcrab_clash_{}", URL_SAFE_NO_PAD.encode(bytes)));
        }
        state.enabled = true;
        state.server = Some(ip.to_string());
        Ok(state.clone())
    }

    pub fn disable(&self) -> ClashSubscriptionState {
        let mut state = self.state.lock().expect("Clash subscription lock poisoned");
        *state = ClashSubscriptionState::default();
        state.clone()
    }

    pub fn yaml(&self, token: &str, port: u16) -> ManagerResult<String> {
        let state = self.state.lock().expect("Clash subscription lock poisoned");
        let (Some(expected), Some(server)) = (&state.token, &state.server) else {
            return Err(invalid_link());
        };
        if !bool::from(Sha256::digest(token.as_bytes()).ct_eq(&Sha256::digest(expected.as_bytes())))
        {
            return Err(invalid_link());
        }
        Ok(format!(
            "mode: rule\nproxies:\n  - name: ProxyCrab\n    type: http\n    server: {server}\n    port: {port}\nrules:\n  - MATCH,ProxyCrab\n"
        ))
    }
}

fn invalid_link() -> ManagerError {
    ManagerError::not_found("Clash subscription link is invalid or disabled")
}

pub fn router(manager: Arc<dyn ProxyCrabManager>) -> Router {
    Router::new()
        .route("/clash.yaml", get(download))
        .with_state(manager)
}

async fn download(
    State(manager): State<Arc<dyn ProxyCrabManager>>,
    RawQuery(query): RawQuery,
) -> Response {
    let token = query.as_deref().and_then(|query| {
        let mut pairs = url::form_urlencoded::parse(query.as_bytes());
        let (key, value) = pairs.next()?;
        (key == "token" && !value.is_empty() && pairs.next().is_none()).then(|| value.into_owned())
    });
    let result = match token {
        Some(token) => manager.clash_subscription_yaml(token).await,
        None => Err(invalid_link()),
    };
    let mut response = match result {
        Ok(yaml) => (
            [
                (CONTENT_TYPE, "application/yaml; charset=utf-8"),
                (
                    CONTENT_DISPOSITION,
                    "attachment; filename=\"ProxyCrab.yaml\"",
                ),
            ],
            yaml,
        )
            .into_response(),
        Err(error) => (
            error.code.http_status(),
            Json(serde_json::json!({"ok": false, "error": error})),
        )
            .into_response(),
    };
    response
        .headers_mut()
        .insert(CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_is_ephemeral_updatable_and_revocable() {
        let subscription = ClashSubscription::default();
        assert!(!subscription.state().enabled);
        assert!(subscription.yaml("missing", 8089).is_err());
        let first = subscription.enable("192.168.1.20").unwrap();
        let token = first.token.unwrap();
        assert_eq!(
            subscription.enable("192.168.1.20").unwrap().token.as_ref(),
            Some(&token)
        );
        assert!(subscription.yaml("wrong", 8089).is_err());
        assert!(
            subscription
                .yaml(&token, 8089)
                .unwrap()
                .contains("server: 192.168.1.20\n    port: 8089")
        );
        subscription.enable("10.0.0.2").unwrap();
        let yaml = subscription.yaml(&token, 9999).unwrap();
        assert!(yaml.contains("server: 10.0.0.2\n    port: 9999"));
        assert!(yaml.ends_with("rules:\n  - MATCH,ProxyCrab\n"));
        assert_eq!(subscription.disable(), ClashSubscriptionState::default());
        assert!(subscription.yaml(&token, 8089).is_err());
        assert_ne!(
            subscription.enable("10.0.0.2").unwrap().token.as_ref(),
            Some(&token)
        );
        assert_eq!(
            ClashSubscription::default().state(),
            ClashSubscriptionState::default()
        );
    }

    #[test]
    fn invalid_addresses_do_not_change_subscription() {
        let subscription = ClashSubscription::default();
        let state = subscription.enable("192.168.1.20").unwrap();
        for server in [
            "127.0.0.1",
            "127.0.0.2",
            "0.0.0.0",
            "0.1.2.3",
            "::1",
            "localhost",
            "224.0.0.1",
            "255.255.255.255",
            "1.2.3.4\nrules: []",
        ] {
            assert!(subscription.enable(server).is_err(), "{server}");
            assert_eq!(subscription.state(), state);
        }
    }
}
