use std::{collections::BTreeSet, net::SocketAddr};

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{
        HeaderValue, Method, StatusCode,
        header::{
            ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
            ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_HEADERS, AUTHORIZATION, HOST,
            ORIGIN, VARY,
        },
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::{
    dto::ManagerError,
    permission::{ManagementCredential, PermissionDenied, api_actions_for_path},
};

use super::{ApiError, permission_denied_response};

pub(super) async fn prepare_management_request(mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    if !path.starts_with("/api/") {
        return next.run(request).await;
    }

    let allowed_origin = request.headers().get(ORIGIN).cloned();
    let is_preflight = request.method() == Method::OPTIONS;
    let mut response = if is_preflight {
        let locally_trusted = is_trusted_local_request(&request);
        if !locally_trusted && !preflight_requests_authorization(&request) {
            return permission_denied_response(PermissionDenied::unauthorized(
                "remote_auth_required",
                "remote management API requests require Authorization",
            ));
        }
        let methods = methods_for_path(&path);
        if methods.is_empty() {
            ApiError(ManagerError::not_found("api endpoint not found")).into_response()
        } else {
            StatusCode::NO_CONTENT.into_response()
        }
    } else {
        match resolve_management_credential(&request) {
            Ok(credential) => {
                request.extensions_mut().insert(credential);
                next.run(request).await
            }
            Err(error) => return permission_denied_response(error),
        }
    };

    if let Some(origin) = allowed_origin {
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response
            .headers_mut()
            .append(VARY, HeaderValue::from_static("Origin"));
    }
    if is_preflight && response.status().is_success() {
        if let Ok(value) = HeaderValue::from_str(&methods_for_path(&path)) {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_METHODS, value);
        }
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type"),
        );
    }
    response
}

pub(super) fn resolve_management_credential(
    request: &Request<Body>,
) -> Result<ManagementCredential, PermissionDenied> {
    let authorization = request
        .headers()
        .get_all(AUTHORIZATION)
        .iter()
        .collect::<Vec<_>>();
    match authorization.as_slice() {
        [] if is_trusted_local_request(request) => Ok(ManagementCredential::LocalLoopback),
        [] => Err(PermissionDenied::unauthorized(
            "remote_auth_required",
            "remote management API requests require Authorization",
        )),
        [value] => value
            .to_str()
            .ok()
            .and_then(parse_bearer)
            .map(|token| ManagementCredential::Bearer(token.to_owned()))
            .ok_or_else(invalid_api_key),
        _ => Err(invalid_api_key()),
    }
}

fn is_trusted_local_request(request: &Request<Body>) -> bool {
    let loopback_peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|peer| peer.0.ip().is_loopback());
    let local_authority = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| is_local_host(authority.host()));
    let local_origin = request.headers().get(ORIGIN).is_none_or(|origin| {
        origin
            .to_str()
            .ok()
            .and_then(|value| value.parse::<axum::http::Uri>().ok())
            .is_some_and(|origin| {
                matches!(origin.scheme_str(), Some("http" | "https" | "tauri"))
                    && origin.host().is_some_and(is_local_host)
            })
    });
    loopback_peer && local_authority && local_origin
}

fn parse_bearer(value: &str) -> Option<&str> {
    let mut parts = value.split_whitespace();
    let (Some(scheme), Some(token), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn invalid_api_key() -> PermissionDenied {
    PermissionDenied::unauthorized(
        "invalid_api_key",
        "Authorization must contain exactly one valid Bearer API key",
    )
}

fn preflight_requests_authorization(request: &Request<Body>) -> bool {
    request
        .headers()
        .get_all(ACCESS_CONTROL_REQUEST_HEADERS)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|name| name.trim().eq_ignore_ascii_case("authorization"))
}

fn methods_for_path(path: &str) -> String {
    api_actions_for_path(path)
        .map(|action| action.method)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_local_host(host: &str) -> bool {
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
        .trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("tauri.localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, header::AUTHORIZATION},
    };

    use crate::permission::ManagementCredential;

    use super::resolve_management_credential;

    fn request(peer: Option<[u8; 4]>, host: &str, authorization: Option<&str>) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/api/proxy/status")
            .header("host", host);
        if let Some(authorization) = authorization {
            request = request.header(AUTHORIZATION, authorization);
        }
        let mut request = request.body(Body::empty()).unwrap();
        if let Some(peer) = peer {
            request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::from((peer, 40000))));
        }
        request
    }

    #[test]
    fn remote_peer_cannot_claim_localhost_identity() {
        let Err(error) =
            resolve_management_credential(&request(Some([192, 0, 2, 1]), "localhost:18089", None))
        else {
            panic!("remote peer must be rejected");
        };
        assert_eq!(error.code, "remote_auth_required");
    }

    #[test]
    fn loopback_peer_with_local_host_uses_local_identity() {
        assert!(matches!(
            resolve_management_credential(&request(Some([127, 0, 0, 1]), "localhost:18089", None,)),
            Ok(ManagementCredential::LocalLoopback)
        ));
    }

    #[test]
    fn bearer_identity_takes_precedence_for_every_peer() {
        for peer in [[127, 0, 0, 1], [192, 0, 2, 1]] {
            assert!(matches!(
                resolve_management_credential(&request(
                    Some(peer),
                    "proxy.example:18089",
                    Some("Bearer pcrab_example"),
                )),
                Ok(ManagementCredential::Bearer(token)) if token == "pcrab_example"
            ));
        }
    }

    #[test]
    fn missing_peer_fails_closed_without_bearer() {
        let Err(error) = resolve_management_credential(&request(None, "localhost:18089", None))
        else {
            panic!("missing peer must be rejected");
        };
        assert_eq!(error.code, "remote_auth_required");
    }

    #[test]
    fn hostile_origin_cannot_use_local_identity() {
        let mut request = request(Some([127, 0, 0, 1]), "localhost:18089", None);
        request
            .headers_mut()
            .insert("origin", "https://attacker.example".parse().unwrap());
        let Err(error) = resolve_management_credential(&request) else {
            panic!("non-local origin must be rejected");
        };
        assert_eq!(error.code, "remote_auth_required");
    }

    #[test]
    fn duplicate_authorization_headers_are_rejected() {
        let mut request = request(
            Some([127, 0, 0, 1]),
            "localhost:18089",
            Some("Bearer first"),
        );
        request
            .headers_mut()
            .append(AUTHORIZATION, "Bearer second".parse().unwrap());
        let Err(error) = resolve_management_credential(&request) else {
            panic!("duplicate credentials must be rejected");
        };
        assert_eq!(error.code, "invalid_api_key");
    }
}
