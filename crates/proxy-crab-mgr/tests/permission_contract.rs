use std::collections::BTreeSet;

use proxy_crab_mgr::permission::{
    PermissionMode, api_actions, api_actions_for_path, find_api_action,
};

fn action_ids(mode: PermissionMode) -> BTreeSet<&'static str> {
    api_actions()
        .iter()
        .filter(|action| action.default_mode == mode)
        .map(|action| action.id)
        .collect()
}

#[test]
fn permission_catalog_contains_every_unique_route_action() {
    let actions = api_actions();
    let ids = actions
        .iter()
        .map(|action| action.id)
        .collect::<BTreeSet<_>>();

    assert_eq!(actions.len(), 67);
    assert_eq!(ids.len(), actions.len());
    assert!(
        actions
            .iter()
            .all(|action| { action.id == format!("{} {}", action.method, action.route_template) })
    );
    assert!(actions.iter().all(|action| !action.method.is_empty()));
    assert!(
        actions
            .iter()
            .all(|action| !action.route_template.is_empty())
    );
}

#[test]
fn permission_catalog_has_the_confirmed_default_matrix() {
    let allow = BTreeSet::from([
        "GET /api/active-session",
        "GET /api/agents.md",
        "GET /api/archived-sessions",
        "GET /api/assets/{*asset_id}",
        "GET /api/breakpoints",
        "GET /api/breakpoints/{id}",
        "GET /api/breakpoints/{id}/body",
        "GET /api/bypass",
        "GET /api/column-scripts",
        "GET /api/column-scripts/{name}",
        "GET /api/config",
        "GET /api/filter-scripts",
        "GET /api/filter-scripts/{name}",
        "GET /api/interceptors",
        "GET /api/interceptors/{kind}/{name}",
        "GET /api/session-har-shares/{id}",
        "GET /api/logs/{id}",
        "GET /api/logs/{id}/body",
        "GET /api/proxy/status",
        "GET /api/routing-script-selection",
        "GET /api/routing-scripts",
        "GET /api/routing-scripts/{name}",
        "GET /api/session-interceptors",
        "GET /api/session-shares/{id}",
        "GET /api/session-view",
        "GET /api/sessions",
        "GET /api/system-logs",
        "POST /api/filter-scripts/{name}/debug",
        "POST /api/logs/ids",
        "POST /api/logs/views",
    ]);
    let approval = BTreeSet::from([
        "POST /api/archived-sessions/{id}/restore",
        "POST /api/assets/{*asset_id}",
        "POST /api/breakpoints/{id}/execute",
        "POST /api/breakpoints/{id}/extend",
        "POST /api/breakpoints/{id}/release",
        "POST /api/column-scripts",
        "POST /api/filter-scripts",
        "POST /api/interceptors",
        "POST /api/proxy/start",
        "POST /api/proxy/stop",
        "POST /api/routing-scripts",
        "POST /api/session-har-shares",
        "POST /api/session-shares",
        "POST /api/sessions",
        "PUT /api/active-session",
        "PUT /api/column-scripts/{name}",
        "PUT /api/filter-scripts/{name}",
        "PUT /api/interceptors/{kind}/{name}",
        "PUT /api/routing-script-selection",
        "PUT /api/routing-scripts/{name}",
        "PUT /api/session-interceptors",
        "PUT /api/session-view",
        "PUT /api/sessions/{id}",
        "PUT /api/sessions/{id}/filter",
        "DELETE /api/session-shares/{id}",
        "DELETE /api/session-har-shares/{id}",
    ]);
    let deny = BTreeSet::from([
        "DELETE /api/archived-sessions/{id}",
        "DELETE /api/bypass",
        "DELETE /api/bypass/{id}",
        "DELETE /api/column-scripts/{name}",
        "DELETE /api/filter-scripts/{name}",
        "DELETE /api/interceptors/{kind}/{name}",
        "DELETE /api/routing-scripts/{name}",
        "DELETE /api/system-logs",
        "GET /api/ca",
        "POST /api/bypass/delete",
        "POST /api/sessions/{id}/archive",
    ]);

    assert_eq!(action_ids(PermissionMode::Allow), allow);
    assert_eq!(action_ids(PermissionMode::Approval), approval);
    assert_eq!(action_ids(PermissionMode::Deny), deny);
}

#[test]
fn permission_lookup_uses_both_method_and_route_template() {
    assert_eq!(
        find_api_action("POST", "/api/filter-scripts/{name}/debug")
            .unwrap()
            .default_mode,
        PermissionMode::Allow
    );
    assert_eq!(
        find_api_action("POST", "/api/breakpoints/{id}/release")
            .unwrap()
            .default_mode,
        PermissionMode::Approval
    );
    assert_eq!(
        find_api_action("GET", "/api/ca").unwrap().default_mode,
        PermissionMode::Deny
    );
    assert!(find_api_action("POST", "/api/ca").is_none());
    assert!(find_api_action("PUT", "/api/config").is_none());
    assert!(find_api_action("PUT", "/api/workspace").is_none());
    assert!(find_api_action("GET", "/api/workspace").is_none());
    assert_eq!(
        find_api_action("POST", "/api/session-har-shares")
            .unwrap()
            .default_mode,
        PermissionMode::Approval
    );
    assert_eq!(
        find_api_action("GET", "/api/session-har-shares/{id}")
            .unwrap()
            .default_mode,
        PermissionMode::Allow
    );
    assert_eq!(
        find_api_action("DELETE", "/api/session-har-shares/{id}")
            .unwrap()
            .default_mode,
        PermissionMode::Approval
    );
    assert!(find_api_action("POST", "/api/logs/export").is_none());
    assert_eq!(
        find_api_action("POST", "/api/session-shares")
            .unwrap()
            .default_mode,
        PermissionMode::Approval
    );
    assert_eq!(
        find_api_action("GET", "/api/session-shares/{id}")
            .unwrap()
            .default_mode,
        PermissionMode::Allow
    );
    assert_eq!(
        find_api_action("DELETE", "/api/session-shares/{id}")
            .unwrap()
            .default_mode,
        PermissionMode::Approval
    );
    assert!(find_api_action("DELETE", "/api/ca").is_none());
    assert!(find_api_action("GET", "/api/logs/123").is_none());

    let methods = api_actions_for_path("/api/column-scripts/example")
        .map(|action| action.method)
        .collect::<BTreeSet<_>>();
    assert_eq!(methods, BTreeSet::from(["DELETE", "GET", "PUT"]));
    assert_eq!(api_actions_for_path("/api/not-found").count(), 0);
}
