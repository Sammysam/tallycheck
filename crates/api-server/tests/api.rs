//! End-to-end tests of the W0 API through the real router (all middleware included).

use api_server::{build_router, AppState};
use app_core::AppError;
use auth::JwtKeys;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use database::Db;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:47800";
type TestResult = Result<(), Box<dyn std::error::Error>>;

struct App {
    _dir: tempfile::TempDir,
    db: Db,
    router: Router,
}

struct Resp {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

impl App {
    async fn new() -> Result<App, AppError> {
        Self::with_frontend(None).await
    }

    async fn with_frontend(frontend: Option<&std::path::Path>) -> Result<App, AppError> {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let db = Db::open(&dir.path().join("t.db")).await?;
        let files = documents::FileStore::new(
            dir.path().join("files"),
            documents::StorageKey::from_bytes(&[5u8; 32]).map_err(AppError::internal)?,
        );
        let state =
            AppState::new(db.clone(), JwtKeys::new(b"test-secret-test-secret-test-secret!"), files, false, 47800);
        let router = build_router(state.with_data_dir(dir.path().to_path_buf()), frontend);
        Ok(App { _dir: dir, db, router })
    }

    async fn call(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Result<Resp, Box<dyn std::error::Error>> {
        self.call_with_host(method, uri, token, body, HOST).await
    }

    async fn call_with_host(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
        host: &str,
    ) -> Result<Resp, Box<dyn std::error::Error>> {
        let mut req = Request::builder().method(method).uri(uri).header("host", host);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let req = match body {
            Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string()))?,
            None => req.body(Body::empty())?,
        };
        let res = self.router.clone().oneshot(req).await?;
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await?.to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        Ok(Resp { status, headers, body })
    }

    /// Creates a user with `role` and completes the mandatory 2FA enrolment; returns a token.
    async fn signed_in(&self, username: &str, role: &str) -> Result<String, Box<dyn std::error::Error>> {
        let role_id: Option<String> = sqlx::query_scalar(r#"SELECT "id" FROM "roles" WHERE "name" = ?1"#)
            .bind(role)
            .fetch_optional(self.db.read())
            .await?;
        auth::users::create_user(
            &self.db,
            auth::users::CreateUser { username: username.into(), password: "password123".into(), role_id },
        )
        .await?;
        let login = self
            .call(Method::POST, "/api/auth/login", None, Some(json!({"username": username, "password": "password123"})))
            .await?;
        let temp = login.body["tempToken"].as_str().ok_or("no tempToken")?.to_string();
        let setup = self.call(Method::GET, "/api/auth/2fa/setup", Some(&temp), None).await?;
        let secret = setup.body["secret"].as_str().ok_or("no secret")?.to_string();
        let code = auth::totp::current_code(&secret).ok_or("no code")?;
        let enabled = self.call(Method::POST, "/api/auth/2fa/enable", Some(&temp), Some(json!({"code": code}))).await?;
        Ok(enabled.body["accessToken"].as_str().ok_or("no accessToken")?.to_string())
    }
}

fn assert_nest_error(r: &Resp, status: u16, message: Value, path: &str) {
    assert_eq!(r.status.as_u16(), status, "{}", r.body);
    assert_eq!(r.body["statusCode"], json!(status));
    assert_eq!(r.body["message"], message);
    assert_eq!(r.body["path"], json!(path));
    assert!(r.body["timestamp"].as_str().is_some_and(|t| t.ends_with('Z')), "{}", r.body);
}

#[tokio::test]
async fn full_login_and_two_factor_flow() -> TestResult {
    let app = App::new().await?;
    auth::users::bootstrap_admin(&app.db, "admin", "admin12345").await?;

    let login = app
        .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "admin", "password": "admin12345"})))
        .await?;
    assert_eq!(login.status, StatusCode::OK);
    assert_eq!(login.body["requiresTwoFactor"], json!(false));
    // D16: without 2FA turned on, the password step signs the user in.
    assert_eq!(login.body["isNewUser"], json!(false));
    let direct = login.body["accessToken"].as_str().ok_or("accessToken")?.to_string();
    let me_direct = app.call(Method::GET, "/api/auth/me", Some(&direct), None).await?;
    assert_eq!(me_direct.status, StatusCode::OK);
    assert_eq!(me_direct.body["isTwoFactorEnabled"], json!(false));
    let temp = login.body["tempToken"].as_str().ok_or("tempToken")?.to_string();

    let early = app.call(Method::GET, "/api/auth/me", Some(&temp), None).await?;
    assert_nest_error(&early, 401, json!("Full authentication required"), "/api/auth/me");

    let setup = app.call(Method::GET, "/api/auth/2fa/setup", Some(&temp), None).await?;
    assert_eq!(setup.status, StatusCode::OK);
    assert!(setup.body["qrCode"].as_str().is_some_and(|q| q.starts_with("data:image/png;base64,")));
    let secret = setup.body["secret"].as_str().ok_or("secret")?.to_string();

    let wrong = app.call(Method::POST, "/api/auth/2fa/enable", Some(&temp), Some(json!({"code": "000000"}))).await?;
    if auth::totp::current_code(&secret).as_deref() != Some("000000") {
        assert_nest_error(&wrong, 401, json!("Invalid 2FA code"), "/api/auth/2fa/enable");
    }
    let code = auth::totp::current_code(&secret).ok_or("code")?;
    let enabled = app.call(Method::POST, "/api/auth/2fa/enable", Some(&temp), Some(json!({"code": code}))).await?;
    assert_eq!(enabled.status, StatusCode::OK);
    let access = enabled.body["accessToken"].as_str().ok_or("accessToken")?.to_string();

    let me = app.call(Method::GET, "/api/auth/me", Some(&access), None).await?;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "admin");
    assert_eq!(me.body["isTwoFactorEnabled"], json!(true));
    assert_eq!(me.body["role"]["name"], "Admin");
    assert_eq!(me.body["role"]["permissions"].as_array().map(Vec::len), Some(17));
    assert!(me.body.get("passwordHash").is_none());

    // Second login: 2FA is required and verify issues the access token.
    let again = app
        .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "admin", "password": "admin12345"})))
        .await?;
    assert_eq!(again.body["requiresTwoFactor"], json!(true));
    let temp2 = again.body["tempToken"].as_str().ok_or("tempToken")?.to_string();
    let verified = app.call(Method::POST, "/api/auth/2fa/verify", Some(&temp2), Some(json!({"code": code}))).await?;
    assert_eq!(verified.status, StatusCode::OK);
    assert!(verified.body["accessToken"].is_string());

    // D2: a password alone can no longer re-enrol 2FA.
    let reenrol = app.call(Method::GET, "/api/auth/2fa/setup", Some(&temp2), None).await?;
    assert_eq!(reenrol.status, StatusCode::FORBIDDEN);

    let no_token = app.call(Method::POST, "/api/auth/2fa/verify", None, Some(json!({"code": code}))).await?;
    assert_nest_error(&no_token, 401, json!("Missing token"), "/api/auth/2fa/verify");
    let bad_token = app.call(Method::POST, "/api/auth/2fa/verify", Some("abc"), Some(json!({"code": code}))).await?;
    assert_nest_error(&bad_token, 401, json!("Invalid token"), "/api/auth/2fa/verify");
    Ok(())
}

#[tokio::test]
async fn failed_logins_are_audited_and_throttled() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("boss", "Admin").await?;
    for _ in 0..5 {
        let r = app
            .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "boss", "password": "nope-nope"})))
            .await?;
        assert_nest_error(&r, 401, json!("Invalid credentials"), "/api/auth/login");
    }
    let locked = app
        .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "boss", "password": "password123"})))
        .await?;
    assert_eq!(locked.status, StatusCode::TOO_MANY_REQUESTS);

    let unknown = app
        .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "ghost", "password": "whatever1"})))
        .await?;
    assert_nest_error(&unknown, 401, json!("Invalid credentials"), "/api/auth/login");
    let empty = app.call(Method::POST, "/api/auth/login", None, Some(json!({}))).await?;
    assert_eq!(empty.status, StatusCode::UNAUTHORIZED);

    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("data")?.iter().filter_map(|e| e["action"].as_str()).collect();
    assert_eq!(actions.iter().filter(|a| **a == "Failed login").count(), 6);
    assert!(actions.contains(&"Logged in"));
    // The audit trail never contains passwords.
    assert!(!log.body.to_string().contains("nope-nope"));
    Ok(())
}

#[tokio::test]
async fn unknown_routes_and_methods_are_nest_404s_before_auth() -> TestResult {
    let app = App::new().await?;
    let r = app.call(Method::GET, "/api/no-such-thing?page=2", None, None).await?;
    assert_nest_error(&r, 404, json!("Cannot GET /api/no-such-thing?page=2"), "/api/no-such-thing?page=2");
    let r = app.call(Method::DELETE, "/api/users", None, None).await?;
    assert_nest_error(&r, 404, json!("Cannot DELETE /api/users"), "/api/users");
    let r = app.call(Method::GET, "/nothing-here", None, None).await?;
    assert_nest_error(&r, 404, json!("Cannot GET /nothing-here"), "/nothing-here");
    Ok(())
}

#[tokio::test]
async fn guards_run_in_reference_order() -> TestResult {
    let app = App::new().await?;
    let viewer = app.signed_in("viewer1", "Viewer").await?;

    let r = app.call(Method::GET, "/api/users", None, None).await?;
    assert_nest_error(&r, 401, json!("Unauthorized"), "/api/users");
    let r = app.call(Method::POST, "/api/users", None, Some(json!({}))).await?;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "auth before validation");
    let r = app.call(Method::POST, "/api/users", Some(&viewer), Some(json!({}))).await?;
    assert_nest_error(&r, 403, json!("Insufficient permissions"), "/api/users");
    let r = app.call(Method::GET, "/api/users", Some("not.a.jwt"), None).await?;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Settings: readable by any signed-in user, writable only with `settings`.
    let r = app.call(Method::GET, "/api/settings", Some(&viewer), None).await?;
    assert_eq!(r.status, StatusCode::OK);
    let r = app.call(Method::PATCH, "/api/settings", Some(&viewer), Some(json!({"signatoryName": "x"}))).await?;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    Ok(())
}

#[tokio::test]
async fn users_endpoints_match_reference_contract() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;

    let invalid = app
        .call(Method::POST, "/api/users", Some(&admin), Some(json!({"username": "ab", "password": "short"})))
        .await?;
    assert_nest_error(
        &invalid,
        400,
        json!([
            "username must be longer than or equal to 3 characters",
            "password must be longer than or equal to 8 characters"
        ]),
        "/api/users",
    );

    let created = app
        .call(
            Method::POST,
            "/api/users",
            Some(&admin),
            Some(json!({"username": "clerk", "password": "longenough", "junk": 1})),
        )
        .await?;
    assert_eq!(created.status, StatusCode::CREATED);
    for hidden in ["passwordHash", "twoFactorSecret", "junk"] {
        assert!(created.body.get(hidden).is_none(), "{hidden} must not be returned");
    }
    assert_eq!(created.body["isActive"], json!(true));
    assert_eq!(created.body["isTwoFactorEnabled"], json!(false));
    assert!(created.body.get("role").is_none(), "reference omits role on create");
    let id = created.body["id"].as_str().ok_or("id")?.to_string();

    let dup = app
        .call(Method::POST, "/api/users", Some(&admin), Some(json!({"username": "clerk", "password": "longenough"})))
        .await?;
    assert_nest_error(&dup, 409, json!("User clerk already exists"), "/api/users");

    let viewer_role: String =
        sqlx::query_scalar(r#"SELECT "id" FROM "roles" WHERE "name"='Viewer'"#).fetch_one(app.db.read()).await?;
    let updated = app
        .call(
            Method::PATCH,
            &format!("/api/users/{id}"),
            Some(&admin),
            Some(json!({"roleId": viewer_role, "isActive": false})),
        )
        .await?;
    assert_eq!(updated.status, StatusCode::OK);
    assert_eq!(updated.body["role"]["name"], "Viewer");
    assert_eq!(updated.body["isActive"], json!(false));

    let reset = app
        .call(
            Method::POST,
            &format!("/api/users/{id}/reset-password"),
            Some(&admin),
            Some(json!({"password": "anotherpass"})),
        )
        .await?;
    assert_eq!(reset.status, StatusCode::CREATED);
    let reset2fa = app.call(Method::POST, &format!("/api/users/{id}/reset-2fa"), Some(&admin), None).await?;
    assert_eq!(reset2fa.status, StatusCode::CREATED);

    let missing = app.call(Method::PATCH, "/api/users/not-a-uuid", Some(&admin), Some(json!({}))).await?;
    assert_nest_error(&missing, 404, json!("User not-a-uuid not found"), "/api/users/not-a-uuid");

    let me_id: String =
        sqlx::query_scalar(r#"SELECT "id" FROM "users" WHERE "username"='admin'"#).fetch_one(app.db.read()).await?;
    let last =
        app.call(Method::PATCH, &format!("/api/users/{me_id}"), Some(&admin), Some(json!({"isActive": false}))).await?;
    assert_nest_error(
        &last,
        400,
        json!("Cannot remove or deactivate the last active Admin"),
        &format!("/api/users/{me_id}"),
    );

    let list = app.call(Method::GET, "/api/users", Some(&admin), None).await?;
    assert_eq!(list.body.as_array().map(Vec::len), Some(2));
    assert_eq!(list.body[0]["username"], "admin", "ordered by createdAt");
    Ok(())
}

#[tokio::test]
async fn inactive_users_are_locked_out_everywhere() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let clerk = app.signed_in("clerk", "Viewer").await?;
    let clerk_id: String =
        sqlx::query_scalar(r#"SELECT "id" FROM "users" WHERE "username"='clerk'"#).fetch_one(app.db.read()).await?;
    app.call(Method::PATCH, &format!("/api/users/{clerk_id}"), Some(&admin), Some(json!({"isActive": false}))).await?;

    let me = app.call(Method::GET, "/api/auth/me", Some(&clerk), None).await?;
    assert_nest_error(&me, 403, json!("Account is inactive"), "/api/auth/me");
    let login = app
        .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "clerk", "password": "password123"})))
        .await?;
    assert_nest_error(&login, 403, json!("Account is inactive"), "/api/auth/login");
    Ok(())
}

#[tokio::test]
async fn roles_endpoints_match_reference_contract() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;

    let catalog = app.call(Method::GET, "/api/roles/permissions", Some(&admin), None).await?;
    assert_eq!(catalog.body.as_array().map(Vec::len), Some(17));
    assert_eq!(catalog.body[7], json!({"key": "invoices:all", "label": "Invoices — see everyone's (not just own)"}));

    let roles = app.call(Method::GET, "/api/roles", Some(&admin), None).await?;
    let names: Vec<&str> = roles.body.as_array().ok_or("roles")?.iter().filter_map(|r| r["name"].as_str()).collect();
    assert_eq!(names, ["Admin", "Accountant", "Viewer"]);
    let admin_id = roles.body[0]["id"].as_str().ok_or("id")?.to_string();

    let bad = app
        .call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "Clerk", "permissions": ["nope"]})))
        .await?;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let created = app
        .call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "Clerk", "permissions": ["invoices"]})))
        .await?;
    assert_eq!(created.status, StatusCode::CREATED);
    assert_eq!(created.body["isSystem"], json!(false));
    assert_eq!(created.body["permissions"], json!(["invoices"]));
    let clerk_id = created.body["id"].as_str().ok_or("id")?.to_string();

    let sys =
        app.call(Method::PATCH, &format!("/api/roles/{admin_id}"), Some(&admin), Some(json!({"name": "Boss"}))).await?;
    assert_nest_error(&sys, 403, json!("The Admin role cannot be modified"), &format!("/api/roles/{admin_id}"));
    let renamed = app
        .call(
            Method::PATCH,
            &format!("/api/roles/{clerk_id}"),
            Some(&admin),
            Some(json!({"permissions": ["invoices", "bills"]})),
        )
        .await?;
    assert_eq!(renamed.body["permissions"], json!(["invoices", "bills"]));
    let gone = app.call(Method::DELETE, &format!("/api/roles/{clerk_id}"), Some(&admin), None).await?;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
    assert_eq!(gone.body, Value::Null);
    Ok(())
}

#[tokio::test]
async fn mutations_are_audited_like_the_reference_interceptor() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "Clerk", "permissions": []}))).await?;
    app.call(Method::PATCH, "/api/settings?x=1", Some(&admin), Some(json!({"signatoryName": "  Jane  "}))).await?;
    // Failed mutations are not recorded.
    app.call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "Clerk", "permissions": []}))).await?;

    let log = app.call(Method::GET, "/api/audit-logs", Some(&admin), None).await?;
    assert_eq!(log.body["page"], json!(1));
    assert_eq!(log.body["limit"], json!(20));
    let data = log.body["data"].as_array().ok_or("data")?;
    assert_eq!(data[0]["action"], "Updated company settings");
    assert_eq!(data[0]["path"], "/api/settings");
    assert_eq!(data[0]["statusCode"], json!(200));
    assert_eq!(data[1]["action"], "Created a role");
    assert_eq!(data[1]["detail"], "Clerk");
    assert_eq!(data[1]["statusCode"], json!(201));
    assert_eq!(data[1]["username"], "admin");
    assert_eq!(data.iter().filter(|e| e["action"] == "Created a role").count(), 1);

    let users = app.call(Method::GET, "/api/audit-logs/users", Some(&admin), None).await?;
    assert_eq!(users.body[0]["username"], "admin");

    let bad = app.call(Method::GET, "/api/audit-logs?page=abc&limit=101", Some(&admin), None).await?;
    assert_nest_error(
        &bad,
        400,
        json!(["page must not be less than 1", "page must be an integer number", "limit must not be greater than 100"]),
        "/api/audit-logs?page=abc&limit=101",
    );
    Ok(())
}

#[tokio::test]
async fn settings_are_trimmed_and_merged_with_defaults() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let r = app
        .call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"signatoryName": "  Jane Doe ", "other": "x"})))
        .await?;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["signatoryName"], "Jane Doe");
    // D25: a fresh install carries no company's details.
    assert_eq!(r.body["signatoryTitle"], "");
    assert_eq!(r.body["payroll.companyName"], "");
    assert!(r.body.get("other").is_none());
    let bad = app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"signatoryTitle": 5}))).await?;
    assert_nest_error(
        &bad,
        400,
        json!(["signatoryTitle must be shorter than or equal to 120 characters", "signatoryTitle must be a string"]),
        "/api/settings",
    );
    Ok(())
}

#[tokio::test]
async fn foreign_host_headers_are_refused_and_headers_are_hardened() -> TestResult {
    let app = App::new().await?;
    let evil = app.call_with_host(Method::GET, "/api/auth/me", None, None, "attacker.example:47800").await?;
    assert_eq!(evil.status, StatusCode::FORBIDDEN);
    let ok = app.call_with_host(Method::GET, "/api/auth/me", None, None, "localhost:47800").await?;
    assert_eq!(ok.status, StatusCode::UNAUTHORIZED);
    assert_eq!(ok.headers.get("x-content-type-options").and_then(|v| v.to_str().ok()), Some("nosniff"));
    assert_eq!(ok.headers.get("x-frame-options").and_then(|v| v.to_str().ok()), Some("DENY"));
    Ok(())
}

#[tokio::test]
async fn frontend_is_served_with_spa_fallback() -> TestResult {
    let site = tempfile::tempdir()?;
    std::fs::write(site.path().join("index.html"), "<!doctype html><title>app</title>")?;
    std::fs::create_dir_all(site.path().join("assets"))?;
    std::fs::write(site.path().join("assets/app.js"), "console.log(1)")?;
    let app = App::with_frontend(Some(site.path())).await?;

    let root = app.call(Method::GET, "/", None, None).await?;
    assert_eq!(root.status, StatusCode::OK);
    assert!(root.body.as_str().is_some_and(|b| b.contains("<title>app</title>")));
    let deep = app.call(Method::GET, "/payroll/123", None, None).await?;
    assert!(deep.body.as_str().is_some_and(|b| b.contains("<title>app</title>")));
    let asset = app.call(Method::GET, "/assets/app.js", None, None).await?;
    assert_eq!(asset.body, json!("console.log(1)"));
    assert_eq!(asset.headers.get("cache-control").and_then(|v| v.to_str().ok()), Some("no-cache"));
    let api = app.call(Method::GET, "/api/no-such-thing", None, None).await?;
    assert_nest_error(&api, 404, json!("Cannot GET /api/no-such-thing"), "/api/no-such-thing");
    Ok(())
}

// ── W1: chart of accounts and ledger ─────────────────────────────────────────

async fn create_account(
    app: &App,
    token: &str,
    code: &str,
    name: &str,
    kind: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let r = app
        .call(Method::POST, "/api/accounts", Some(token), Some(json!({"code": code, "name": name, "type": kind})))
        .await?;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    Ok(r.body["id"].as_str().ok_or("id")?.to_string())
}

#[tokio::test]
async fn ledger_posting_reversal_and_balances_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let cash = create_account(&app, &admin, "1000", "Cash", "ASSET").await?;
    let equity = create_account(&app, &admin, "3000", "Equity", "EQUITY").await?;
    let expense = create_account(&app, &admin, "6000", "Expenses", "EXPENSE").await?;

    let open = app
        .call(
            Method::POST,
            "/api/ledger/entries",
            Some(&admin),
            Some(json!({
            "date": "2026-04-01", "description": "Opening", "lines": [
                {"accountId": cash, "debit": 1000.5, "credit": 0},
                {"accountId": equity, "debit": 0, "credit": 1000.5}]})),
        )
        .await?;
    assert_eq!(open.status, StatusCode::CREATED);
    assert_eq!(open.body["entryNumber"], "JE-000001");
    assert_eq!(open.body["lines"][0]["debit"], "1000.50");
    assert!(open.body["lines"][0].get("account").is_none());

    let spend = app
        .call(
            Method::POST,
            "/api/ledger/entries",
            Some(&admin),
            Some(json!({
            "date": "2026-04-15", "description": "Supplies", "lines": [
                {"accountId": expense, "debit": 250, "credit": 0},
                {"accountId": cash, "debit": 0, "credit": 250}]})),
        )
        .await?;
    let spend_id = spend.body["id"].as_str().ok_or("id")?.to_string();

    let unbalanced = app
        .call(
            Method::POST,
            "/api/ledger/entries",
            Some(&admin),
            Some(json!({
            "date": "2026-04-15", "description": "x", "lines": [
                {"accountId": cash, "debit": 100.1, "credit": 0},
                {"accountId": equity, "debit": 0, "credit": 100}]})),
        )
        .await?;
    assert_nest_error(
        &unbalanced,
        400,
        json!("Journal entry is not balanced: debits 100.1 ≠ credits 100"),
        "/api/ledger/entries",
    );

    let unknown = app
        .call(
            Method::POST,
            "/api/ledger/entries",
            Some(&admin),
            Some(json!({
            "date": "2026-04-15", "description": "x", "lines": [
                {"accountId": "7c9e6679-7425-40de-944b-e07fc1f90ae7", "debit": 1, "credit": 0},
                {"accountId": equity, "debit": 0, "credit": 1}]})),
        )
        .await?;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);

    let rev = app.call(Method::POST, &format!("/api/ledger/entries/{spend_id}/reverse"), Some(&admin), None).await?;
    assert_eq!(rev.status, StatusCode::CREATED);
    assert_eq!(rev.body["entryNumber"], "JE-000003", "failed posts consumed no numbers");
    assert_eq!(rev.body["date"], json!(app_core::time::today_local().to_string()));
    let again = app.call(Method::POST, &format!("/api/ledger/entries/{spend_id}/reverse"), Some(&admin), None).await?;
    assert_nest_error(
        &again,
        400,
        json!("This entry has already been reversed"),
        &format!("/api/ledger/entries/{spend_id}/reverse"),
    );

    let bal = app.call(Method::GET, &format!("/api/ledger/balance/{cash}"), Some(&admin), None).await?;
    assert_eq!(bal.body, json!({"accountId": cash, "debit": 1250.5, "credit": 250, "balance": 1000.5}));
    let exp = app.call(Method::GET, &format!("/api/ledger/balance/{expense}"), Some(&admin), None).await?;
    assert_eq!(exp.body["balance"], json!(0));

    let tb = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    let rows = tb.body.as_array().ok_or("rows")?;
    let total = |k: &str| rows.iter().map(|r| r[k].as_f64().unwrap_or(0.0)).sum::<f64>();
    assert!((total("debit") - total("credit")).abs() < 1e-9);
    let bad_date = app.call(Method::GET, "/api/ledger/trial-balance?asOf=30-04-2026", Some(&admin), None).await?;
    assert_eq!(bad_date.status, StatusCode::BAD_REQUEST);

    let entries = app.call(Method::GET, "/api/ledger/entries", Some(&admin), None).await?;
    let list = entries.body.as_array().ok_or("entries")?;
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["entryNumber"], "JE-000003", "newest date first");
    assert!(list[0]["lines"][0]["account"]["code"].is_string(), "list carries accounts");
    let reversed = list.iter().find(|e| e["id"] == json!(spend_id)).ok_or("spend")?;
    assert_eq!(reversed["status"], "REVERSED");

    // Accounts used by the ledger cannot be deleted; there is no route that deletes entries.
    let del = app.call(Method::DELETE, &format!("/api/accounts/{cash}"), Some(&admin), None).await?;
    assert_eq!(del.status, StatusCode::BAD_REQUEST);
    let no_delete = app.call(Method::DELETE, &format!("/api/ledger/entries/{spend_id}"), Some(&admin), None).await?;
    assert_eq!(no_delete.status, StatusCode::NOT_FOUND);

    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("data")?.iter().filter_map(|e| e["action"].as_str()).collect();
    assert!(actions.contains(&"Reversed a ledger entry"));
    assert_eq!(actions.iter().filter(|a| **a == "Created a ledger entry").count(), 2);
    assert_eq!(actions.iter().filter(|a| **a == "Created an account").count(), 3);
    Ok(())
}

#[tokio::test]
async fn account_and_ledger_permissions_match_reference() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let viewer = app.signed_in("viewer", "Viewer").await?;
    create_account(&app, &admin, "1000", "Cash", "ASSET").await?;

    // Reading the chart is open to every signed-in user; changing it needs `accounts`.
    let list = app.call(Method::GET, "/api/accounts", Some(&viewer), None).await?;
    assert_eq!(list.status, StatusCode::OK);
    let create = app
        .call(Method::POST, "/api/accounts", Some(&viewer), Some(json!({"code": "2", "name": "x", "type": "ASSET"})))
        .await?;
    assert_eq!(create.status, StatusCode::FORBIDDEN);
    let seed = app.call(Method::POST, "/api/accounts/seed", Some(&viewer), None).await?;
    assert_eq!(seed.status, StatusCode::FORBIDDEN);
    // The seeded Viewer role holds `ledger`, so it may read (and post) journal entries.
    let entries = app.call(Method::GET, "/api/ledger/entries", Some(&viewer), None).await?;
    assert_eq!(entries.status, StatusCode::OK);
    let anon = app.call(Method::GET, "/api/accounts", None, None).await?;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);

    let seeded = app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    assert_eq!(seeded.status, StatusCode::NO_CONTENT);
    Ok(())
}

// ── W2: operational accounting ───────────────────────────────────────────────

#[tokio::test]
async fn invoice_to_payment_flow_with_bills_and_expenses() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let seeded = app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    assert_eq!(seeded.status, StatusCode::NO_CONTENT);
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (cash, revenue, expense) = (id_of("1000")?, id_of("4000")?, id_of("6000")?);

    let cust = app
        .call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Acme", "email": "a@acme.lk"})))
        .await?;
    let cust_id = cust.body["id"].as_str().ok_or("cust")?.to_string();
    // D11: the form sends "" for an email left empty; that means "no email".
    let blank = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "X", "email": ""}))).await?;
    assert_eq!(blank.status, StatusCode::CREATED);
    assert_eq!(blank.body["email"], Value::Null);
    let bad =
        app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "X", "email": "nope"}))).await?;
    assert_nest_error(&bad, 400, json!(["email must be an email"]), "/api/customers");

    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({
            "customerId": cust_id, "date": "2026-04-01", "paymentTerms": "NET_30",
            "lines": [{"description": "Design", "quantity": 2.5, "unitPrice": 1000.25, "accountId": revenue}]})),
        )
        .await?;
    assert_eq!(inv.status, StatusCode::CREATED);
    assert_eq!(inv.body["invoiceNumber"], "INV-9000");
    assert_eq!(inv.body["dueDate"], "2026-05-01");
    assert_eq!(inv.body["lines"][0]["amount"], json!(2500.63));
    assert!(inv.body.get("customer").is_none());
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();

    let posted = app
        .call(Method::PATCH, &format!("/api/invoices/{inv_id}/status"), Some(&admin), Some(json!({"status": "POSTED"})))
        .await?;
    assert_eq!(posted.body["status"], "POSTED");
    assert_eq!(posted.body["customer"]["name"], "Acme");
    let pay = app
        .call(
            Method::POST,
            "/api/payments",
            Some(&admin),
            Some(json!({
            "type": "AR", "invoiceId": inv_id, "date": "2026-04-25", "amount": 2500.63,
            "paymentMethod": "BANK_TRANSFER", "paymentAccountId": cash})),
        )
        .await?;
    assert_eq!(pay.status, StatusCode::CREATED);
    assert_eq!(pay.body["amount"], json!(2500.63));
    let after = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!((after.body["status"].as_str(), after.body["amountPaid"].as_str()), (Some("PAID"), Some("2500.63")));

    let vendor = app.call(Method::POST, "/api/vendors", Some(&admin), Some(json!({"name": "Paper Co"}))).await?;
    let bill = app
        .call(
            Method::POST,
            "/api/bills",
            Some(&admin),
            Some(json!({
            "vendorId": vendor.body["id"], "date": "2026-04-03",
            "lines": [{"description": "Paper", "quantity": 3, "unitPrice": 12.5, "accountId": expense}]})),
        )
        .await?;
    let bill_id = bill.body["id"].as_str().ok_or_else(|| format!("bill: {} {}", bill.status, bill.body))?.to_string();
    let bill_post = app.call(Method::POST, &format!("/api/bills/{bill_id}/post"), Some(&admin), None).await?;
    assert_eq!(bill_post.body["status"], "POSTED");
    let exp = app
        .call(
            Method::POST,
            "/api/expenses",
            Some(&admin),
            Some(json!({
            "date": "2026-04-08", "description": "Taxi", "amount": 15.25,
            "expenseAccountId": expense, "paymentAccountId": cash})),
        )
        .await?;
    let exp_id = exp.body["id"].as_str().ok_or("exp")?.to_string();
    app.call(Method::POST, &format!("/api/expenses/{exp_id}/post"), Some(&admin), None).await?;

    let tb = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    let rows = tb.body.as_array().ok_or("tb")?;
    let bal = |code: &str| rows.iter().find(|r| r["account"]["code"] == code).map(|r| r["balance"].clone());
    assert_eq!(bal("1000"), Some(json!(2485.38)));
    assert_eq!(bal("1100"), Some(json!(0)));
    assert_eq!(bal("2000"), Some(json!(37.5)));
    assert_eq!(bal("6000"), Some(json!(52.75)));
    Ok(())
}

#[tokio::test]
async fn invoice_scope_and_operation_permissions() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let viewer = app.signed_in("viewer", "Viewer").await?;
    let role = app
        .call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "Sales", "permissions": ["invoices"]})))
        .await?;
    let role_id = role.body["id"].as_str().ok_or("role")?.to_string();
    let u = app
        .call(
            Method::POST,
            "/api/users",
            Some(&admin),
            Some(json!({"username": "sales1", "password": "salespass1", "roleId": role_id})),
        )
        .await?;
    assert_eq!(u.status, StatusCode::CREATED);
    // Sign sales1 in (users created over the API still enrol 2FA at first login).
    let login = app
        .call(Method::POST, "/api/auth/login", None, Some(json!({"username": "sales1", "password": "salespass1"})))
        .await?;
    let temp = login.body["tempToken"].as_str().ok_or("temp")?.to_string();
    let secret = app.call(Method::GET, "/api/auth/2fa/setup", Some(&temp), None).await?.body["secret"]
        .as_str()
        .ok_or("secret")?
        .to_string();
    let code = auth::totp::current_code(&secret).ok_or("code")?;
    let sales = app.call(Method::POST, "/api/auth/2fa/enable", Some(&temp), Some(json!({"code": code}))).await?.body
        ["accessToken"]
        .as_str()
        .ok_or("token")?
        .to_string();

    let theirs = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"billToName": "A", "date": "2026-04-01", "lines": [{"description": "x"}]})),
        )
        .await?;
    let theirs_id = theirs.body["id"].as_str().ok_or("id")?.to_string();
    app.call(
        Method::POST,
        "/api/invoices",
        Some(&sales),
        Some(json!({"billToName": "B", "date": "2026-04-02", "lines": [{"description": "y"}]})),
    )
    .await?;

    let own = app.call(Method::GET, "/api/invoices", Some(&sales), None).await?;
    assert_eq!(own.body["total"], json!(1));
    let hidden = app.call(Method::GET, &format!("/api/invoices/{theirs_id}"), Some(&sales), None).await?;
    assert_nest_error(
        &hidden,
        404,
        json!(format!("Invoice {theirs_id} not found")),
        &format!("/api/invoices/{theirs_id}"),
    );
    let all = app.call(Method::GET, "/api/invoices", Some(&admin), None).await?;
    assert_eq!(all.body["total"], json!(2));

    let pay = app.call(Method::GET, "/api/payments", Some(&sales), None).await?;
    assert_eq!(pay.status, StatusCode::FORBIDDEN);
    let cust = app.call(Method::GET, "/api/customers", Some(&viewer), None).await?;
    assert_eq!(cust.status, StatusCode::OK, "reading customers is open to every signed-in user");
    let vend = app.call(Method::GET, "/api/vendors", Some(&viewer), None).await?;
    assert_eq!(vend.status, StatusCode::FORBIDDEN);
    Ok(())
}

// ── W3: reports ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn reports_follow_reference_query_rules_and_permissions() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let viewer = app.signed_in("viewer", "Viewer").await?;

    let pl = app.call(Method::GET, "/api/reports/profit-loss", Some(&admin), None).await?;
    assert_eq!(pl.status, StatusCode::OK);
    let today = app_core::time::today_local().to_string();
    assert_eq!(pl.body["dateTo"], json!(today));
    assert_eq!(pl.body["dateFrom"], json!(format!("{}-01-01", &today[..4])));
    assert_eq!(pl.body["netIncome"], json!(0));
    assert!(pl.body["operatingExpenses"].as_array().is_some_and(|a| !a.is_empty()), "zero-balance accounts are listed");

    let open =
        app.call(Method::GET, "/api/reports/profit-loss?dateFrom=&dateTo=2026-04-30", Some(&admin), None).await?;
    assert_eq!(open.body["dateFrom"], json!(""), "an empty bound is kept and means unbounded");
    let bad = app.call(Method::GET, "/api/reports/balance-sheet?asOf=30/04/2026", Some(&admin), None).await?;
    assert_nest_error(
        &bad,
        400,
        json!(["asOf must be a valid date in YYYY-MM-DD format"]),
        "/api/reports/balance-sheet?asOf=30/04/2026",
    );
    let bs = app.call(Method::GET, "/api/reports/balance-sheet?asOf=2026-04-30", Some(&admin), None).await?;
    assert_eq!(bs.body["balanced"], json!(true));
    let ar = app.call(Method::GET, "/api/reports/ar-aging?asOf=2026-04-30", Some(&admin), None).await?;
    for bucket in ["current", "days1_30", "days31_60", "days61_90", "days90plus"] {
        assert_eq!(ar.body[bucket], json!({"items": [], "total": 0}));
    }
    assert_eq!(ar.body["grandTotal"], json!(0));

    // Viewer holds `reports`; a role without it is refused.
    let v = app.call(Method::GET, "/api/reports/ap-aging", Some(&viewer), None).await?;
    assert_eq!(v.status, StatusCode::OK);
    let role = app
        .call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "NoReports", "permissions": ["invoices"]})))
        .await?;
    let role_id = role.body["id"].as_str().ok_or("role")?.to_string();
    let viewer_id: String =
        sqlx::query_scalar(r#"SELECT "id" FROM "users" WHERE "username"='viewer'"#).fetch_one(app.db.read()).await?;
    app.call(Method::PATCH, &format!("/api/users/{viewer_id}"), Some(&admin), Some(json!({"roleId": role_id}))).await?;
    let denied = app.call(Method::GET, "/api/reports/ap-aging", Some(&viewer), None).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    Ok(())
}

// ── W4: employees, photos, documents, salaries ──────────────────────────────

const BOUNDARY: &str = "----w4testboundary";

/// A multipart/form-data body: text fields plus an optional `(field, filename, type, bytes)` file.
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &str, &str, &[u8])>) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, v) in fields {
        out.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes(),
        );
    }
    if let Some((field, name, mime, bytes)) = file {
        out.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\nContent-Type: {mime}\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

impl App {
    async fn upload(&self, uri: &str, token: &str, body: Vec<u8>) -> Result<Resp, Box<dyn std::error::Error>> {
        let req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("host", HOST)
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", format!("multipart/form-data; boundary={BOUNDARY}"))
            .body(Body::from(body))?;
        let res = self.router.clone().oneshot(req).await?;
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await?.to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok(Resp { status, headers, body })
    }

    async fn download(
        &self,
        uri: &str,
        token: &str,
    ) -> Result<(StatusCode, axum::http::HeaderMap, Vec<u8>), Box<dyn std::error::Error>> {
        let req = Request::builder()
            .uri(uri)
            .header("host", HOST)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())?;
        let res = self.router.clone().oneshot(req).await?;
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await?.to_bytes().to_vec();
        Ok((status, headers, bytes))
    }
}

#[tokio::test]
async fn employee_files_upload_encrypted_and_download_with_reference_headers() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("boss", "Admin").await?;
    let created = app
        .call(
            Method::POST,
            "/api/employees",
            Some(&admin),
            Some(json!({"employeeId": " CEN-001 ", "name": "Asha", "phone": ""})),
        )
        .await?;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["employeeId"], "CEN-001");
    assert_eq!(created.body["phone"], Value::Null);
    assert_eq!(created.body["staffLoanBalance"], "0.00");
    let id = created.body["id"].as_str().ok_or("id")?.to_string();

    // Photo: stored encrypted, served with no-store and no disposition.
    let png = b"\x89PNG\r\n\x1a\n fake image";
    let up = app
        .upload(
            &format!("/api/employees/{id}/photo"),
            &admin,
            multipart(&[], Some(("file", "Me.PNG", "image/png", png))),
        )
        .await?;
    assert_eq!(up.status, StatusCode::CREATED, "{}", up.body);
    let key = up.body["photoKey"].as_str().ok_or("photoKey")?.to_string();
    assert!(key.starts_with(&format!("employees/{id}/photo/")) && key.ends_with(".png"), "{key}");
    let on_disk = std::fs::read(app._dir.path().join("files").join(&key))?;
    assert_eq!(&on_disk[..4], b"QBE1");
    let (status, headers, bytes) = app.download(&format!("/api/employees/{id}/photo"), &admin).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, png);
    assert_eq!(headers["content-type"], "image/png");
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(headers.get("content-disposition").is_none());

    let wrong = app
        .upload(
            &format!("/api/employees/{id}/photo"),
            &admin,
            multipart(&[], Some(("file", "a.txt", "text/plain", b"x"))),
        )
        .await?;
    assert_nest_error(
        &wrong,
        400,
        json!("Profile photo must be a JPG, PNG or WebP image"),
        &format!("/api/employees/{id}/photo"),
    );
    let big = vec![0u8; 5 * 1024 * 1024 + 1];
    let too_large = app
        .upload(
            &format!("/api/employees/{id}/photo"),
            &admin,
            multipart(&[], Some(("file", "big.jpg", "image/jpeg", &big))),
        )
        .await?;
    assert_nest_error(&too_large, 413, json!("File too large"), &format!("/api/employees/{id}/photo"));
    let other_field = app
        .upload(
            &format!("/api/employees/{id}/photo"),
            &admin,
            multipart(&[], Some(("avatar", "a.png", "image/png", b"x"))),
        )
        .await?;
    assert_nest_error(&other_field, 400, json!("Unexpected field"), &format!("/api/employees/{id}/photo"));

    // Documents: validation before the file check; download headers as the reference.
    let docs = format!("/api/employees/{id}/documents");
    let no_title =
        app.upload(&docs, &admin, multipart(&[], Some(("file", "a.pdf", "application/pdf", b"%PDF")))).await?;
    assert_eq!(no_title.status, StatusCode::BAD_REQUEST);
    assert_eq!(no_title.body["message"][1], "Give the document a title");
    let doc = app
        .upload(
            &docs,
            &admin,
            multipart(
                &[("title", " Visa copy "), ("category", "VISA")],
                Some(("file", "Visa 2026 (copy).pdf", "application/pdf", b"%PDF-1.4 visa")),
            ),
        )
        .await?;
    assert_eq!(doc.status, StatusCode::CREATED, "{}", doc.body);
    assert_eq!(doc.body["title"], "Visa copy");
    assert_eq!(doc.body["size"], json!(13));
    assert_eq!(doc.body["uploadedByName"], "boss");
    let doc_id = doc.body["id"].as_str().ok_or("doc id")?.to_string();
    let (status, headers, bytes) = app.download(&format!("{docs}/{doc_id}/download"), &admin).await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, b"%PDF-1.4 visa");
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"Visa 2026 (copy).pdf\"; filename*=UTF-8''Visa%202026%20(copy).pdf"
    );
    let (_, headers, _) = app.download(&format!("{docs}/{doc_id}/download?inline=1"), &admin).await?;
    assert!(headers["content-disposition"].to_str()?.starts_with("inline; "));

    let listed = app.call(Method::GET, &docs, Some(&admin), None).await?;
    assert_eq!(listed.body[0]["size"], "13", "bigint reads back as a string");
    let bad_uuid = app.call(Method::GET, "/api/employees/abc/documents", Some(&admin), None).await?;
    assert_nest_error(&bad_uuid, 400, json!("Validation failed (uuid is expected)"), "/api/employees/abc/documents");

    // Deleting the employee removes its files.
    let gone = app.call(Method::DELETE, &format!("/api/employees/{id}"), Some(&admin), None).await?;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
    assert!(!app._dir.path().join("files").join(&key).exists());
    Ok(())
}

#[tokio::test]
async fn bank_details_and_salaries_need_payroll_permission() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("boss", "Admin").await?;
    let hr_role = app
        .call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "HR", "permissions": ["employees"]})))
        .await?;
    assert_eq!(hr_role.status, StatusCode::CREATED);
    let hr = app.signed_in("hr1", "HR").await?;
    let viewer = app.signed_in("viewer1", "Viewer").await?;

    let e = app
        .call(Method::POST, "/api/employees", Some(&hr), Some(json!({"employeeId": "CEN-9", "name": "Kamal"})))
        .await?;
    assert_eq!(e.status, StatusCode::CREATED);
    assert!(e.body.get("bankName").is_none() && e.body.get("staffLoanBalance").is_none(), "redacted for HR");
    let id = e.body["id"].as_str().ok_or("id")?.to_string();

    let bank = app
        .call(
            Method::PATCH,
            &format!("/api/payroll/employees/{id}/bank"),
            Some(&admin),
            Some(json!({"bankName": " BOC ", "staffLoanBalance": "1500.25"})),
        )
        .await?;
    assert_eq!(bank.status, StatusCode::OK, "{}", bank.body);
    assert_eq!(bank.body["bank"]["bankName"], "BOC");
    assert_eq!(bank.body["staffLoanBalance"], json!(1500.25));

    let sal = app
        .call(
            Method::POST,
            &format!("/api/payroll/employees/{id}/salaries"),
            Some(&admin),
            Some(json!({"effectiveFrom": "2026-04-01", "basic": "50000", "bra2005": 1000})),
        )
        .await?;
    assert_eq!(sal.status, StatusCode::CREATED, "{}", sal.body);
    assert_eq!(sal.body["monthlyTotal"], json!(51000));
    assert_eq!(sal.body["welfareDeduction"], json!(300));
    assert_eq!(sal.body["createdByName"], "boss");

    let as_admin = app.call(Method::GET, &format!("/api/employees/{id}"), Some(&admin), None).await?;
    assert_eq!(as_admin.body["staffLoanBalance"], "1500.25");
    let as_hr = app.call(Method::GET, &format!("/api/employees/{id}"), Some(&hr), None).await?;
    assert!(as_hr.body.get("bankName").is_none());
    assert_eq!(as_hr.body["documents"], json!([]));
    let payroll_hr = app.call(Method::GET, &format!("/api/payroll/employees/{id}"), Some(&hr), None).await?;
    assert_nest_error(&payroll_hr, 403, json!("Insufficient permissions"), &format!("/api/payroll/employees/{id}"));
    let viewer_list = app.call(Method::GET, "/api/employees", Some(&viewer), None).await?;
    assert_nest_error(&viewer_list, 403, json!("Insufficient permissions"), "/api/employees");

    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("data")?.iter().filter_map(|x| x["action"].as_str()).collect();
    assert!(actions.contains(&"Added a salary / increment"), "{actions:?}");
    assert!(actions.contains(&"Updated an employee's bank details"), "{actions:?}");
    Ok(())
}

// ── W5: payroll runs ─────────────────────────────────────────────────────────

#[tokio::test]
async fn payroll_runs_need_the_payroll_permission_and_answer_201() -> TestResult {
    let app = App::new().await?;
    let accountant = app.signed_in("acc1", "Accountant").await?;
    let viewer = app.signed_in("viewer1", "Viewer").await?;
    let admin = app.signed_in("boss", "Admin").await?;
    app.call(Method::POST, "/api/roles", Some(&admin), Some(json!({"name": "HR", "permissions": ["employees"]})))
        .await?;
    let hr = app.signed_in("hr1", "HR").await?;

    for token in [&viewer, &hr] {
        let r = app.call(Method::GET, "/api/payroll/runs", Some(token), None).await?;
        assert_nest_error(&r, 403, json!("Insufficient permissions"), "/api/payroll/runs");
    }
    let bad =
        app.call(Method::POST, "/api/payroll/runs", Some(&accountant), Some(json!({"period": "2026-13"}))).await?;
    assert_nest_error(&bad, 400, json!(["period must be YYYY-MM"]), "/api/payroll/runs");

    let e = app
        .call(Method::POST, "/api/employees", Some(&accountant), Some(json!({"employeeId": "CEN-1", "name": "Asha"})))
        .await?;
    let emp = e.body["id"].as_str().ok_or("employee id")?.to_string();
    app.call(
        Method::POST,
        &format!("/api/payroll/employees/{emp}/salaries"),
        Some(&accountant),
        Some(json!({"effectiveFrom": "2026-01-01", "basic": 40500, "bra2005": 1000, "bra2016": 2500, "vehicleAllowance": 15000, "otherAllowance": 24850})),
    )
    .await?;
    let run =
        app.call(Method::POST, "/api/payroll/runs", Some(&accountant), Some(json!({"period": "2026-04"}))).await?;
    assert_eq!(run.status, StatusCode::CREATED, "{}", run.body);
    assert_eq!(run.body["netTotal"], "80005.00");
    assert_eq!(run.body["owed"]["EPF"]["total"], json!(8800));
    let id = run.body["id"].as_str().ok_or("run id")?.to_string();

    let posted = app.call(Method::POST, &format!("/api/payroll/runs/{id}/post"), Some(&accountant), None).await?;
    assert_eq!(posted.status, StatusCode::CREATED, "{}", posted.body);
    let del = app.call(Method::DELETE, &format!("/api/payroll/runs/{id}"), Some(&accountant), None).await?;
    assert_nest_error(
        &del,
        400,
        json!("This run is POSTED and can no longer be changed — void it to redo the month"),
        &format!("/api/payroll/runs/{id}"),
    );
    let not_uuid = app.call(Method::POST, "/api/payroll/runs/abc/void", Some(&accountant), None).await?;
    assert_nest_error(&not_uuid, 400, json!("Validation failed (uuid is expected)"), "/api/payroll/runs/abc/void");

    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("data")?.iter().filter_map(|x| x["action"].as_str()).collect();
    assert!(actions.contains(&"Created a payroll run"), "{actions:?}");
    // Reference quirk reproduced on purpose: its payroll-specific wording is unreachable.
    assert!(actions.contains(&"Posted a payroll"), "{actions:?}");
    Ok(())
}

// ── W6: backup, restore and LAN access ──────────────────────────────────────

impl App {
    /// A request arriving from `peer` (what `into_make_service_with_connect_info` records).
    async fn call_from(
        &self,
        peer: &str,
        host: &str,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Result<Resp, Box<dyn std::error::Error>> {
        let addr: std::net::SocketAddr = peer.parse()?;
        let mut req = Request::builder().method(method).uri(uri).header("host", host);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let mut req = match body {
            Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string()))?,
            None => req.body(Body::empty())?,
        };
        req.extensions_mut().insert(axum::extract::ConnectInfo(addr));
        let res = self.router.clone().oneshot(req).await?;
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await?.to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok(Resp { status, headers, body })
    }

    async fn local(
        &self,
        method: Method,
        uri: &str,
        token: &str,
        body: Option<Value>,
    ) -> Result<Resp, Box<dyn std::error::Error>> {
        self.call_from("127.0.0.1:50000", HOST, method, uri, Some(token), body).await
    }
}

#[tokio::test]
async fn backup_and_restore_are_for_a_local_administrator_only() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("boss", "Admin").await?;
    let accountant = app.signed_in("acc1", "Accountant").await?;
    let r = app.local(Method::GET, "/api/system/backup", &accountant, None).await?;
    assert_nest_error(
        &r,
        403,
        json!("Backup and restore are only available to an Administrator on the main PC"),
        "/api/system/backup",
    );
    // An administrator, but over the network: refused.
    let remote =
        app.call_from("192.168.1.50:50000", HOST, Method::GET, "/api/system/backup", Some(&admin), None).await?;
    assert_eq!(remote.status, StatusCode::FORBIDDEN);
    let no_peer = app.call(Method::GET, "/api/system/backup", Some(&admin), None).await?;
    assert_eq!(no_peer.status, StatusCode::FORBIDDEN, "unknown origin counts as remote");
    let ok = app.local(Method::GET, "/api/system/backup", &admin, None).await?;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert_eq!(ok.body["passwordSet"], json!(false));
    Ok(())
}

#[tokio::test]
async fn backup_verify_stage_and_cancel_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("boss", "Admin").await?;
    let dest = tempfile::tempdir()?;
    let folder = dest.path().to_string_lossy().to_string();

    let no_password =
        app.local(Method::POST, "/api/system/backup/run", &admin, Some(json!({"folder": folder}))).await?;
    assert_nest_error(&no_password, 400, json!("Set a backup password first"), "/api/system/backup/run");
    let short =
        app.local(Method::POST, "/api/system/backup/password", &admin, Some(json!({"password": "short"}))).await?;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);
    let set = app
        .local(Method::POST, "/api/system/backup/password", &admin, Some(json!({"password": "office backup 2026"})))
        .await?;
    assert_eq!(set.status, StatusCode::NO_CONTENT);
    let change = app
        .local(
            Method::POST,
            "/api/system/backup/password",
            &admin,
            Some(json!({"password": "another long one", "currentPassword": "wrong"})),
        )
        .await?;
    assert_nest_error(&change, 400, json!("The current backup password is not correct"), "/api/system/backup/password");

    let made = app.local(Method::POST, "/api/system/backup/run", &admin, Some(json!({"folder": folder}))).await?;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let file = made.body["file"].as_str().ok_or("file")?.to_string();
    assert!(std::path::Path::new(&file).exists());
    assert_eq!(made.body["counts"]["users"], json!(1));

    let checked = app.local(Method::POST, "/api/system/restore/verify", &admin, Some(json!({"file": file}))).await?;
    assert_eq!(checked.status, StatusCode::OK, "{}", checked.body);
    let wrong = app
        .local(
            Method::POST,
            "/api/system/restore/verify",
            &admin,
            Some(json!({"file": file, "password": "not it at all"})),
        )
        .await?;
    assert_nest_error(
        &wrong,
        400,
        json!("Wrong backup password, or the backup file is damaged"),
        "/api/system/restore/verify",
    );

    let unconfirmed = app.local(Method::POST, "/api/system/restore/stage", &admin, Some(json!({"file": file}))).await?;
    assert_eq!(unconfirmed.status, StatusCode::BAD_REQUEST);
    let staged = app
        .local(Method::POST, "/api/system/restore/stage", &admin, Some(json!({"file": file, "confirm": "RESTORE"})))
        .await?;
    assert_eq!(staged.status, StatusCode::CREATED, "{}", staged.body);
    let status = app.local(Method::GET, "/api/system/backup", &admin, None).await?;
    assert!(status.body["stagedRestore"].is_object());
    assert!(status.body["status"]["lastSuccessAt"].is_string());
    let cancel = app.local(Method::DELETE, "/api/system/restore/stage", &admin, None).await?;
    assert_eq!(cancel.status, StatusCode::NO_CONTENT);
    let status = app.local(Method::GET, "/api/system/backup", &admin, None).await?;
    assert!(status.body["stagedRestore"].is_null());

    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("data")?.iter().filter_map(|x| x["action"].as_str()).collect();
    for a in
        ["Made a backup", "Changed the backup password", "Staged a restore from a backup", "Cancelled a staged restore"]
    {
        assert!(actions.contains(&a), "{a} missing from {actions:?}");
    }
    Ok(())
}

#[tokio::test]
async fn lan_mode_accepts_office_clients_and_refuses_the_rest() -> TestResult {
    let dir = tempfile::tempdir()?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    let files = documents::FileStore::new(
        dir.path().join("files"),
        documents::StorageKey::from_bytes(&[5u8; 32]).map_err(AppError::internal)?,
    );
    let state = AppState::new(db.clone(), JwtKeys::new(b"test-secret-test-secret-test-secret!"), files, true, 47800)
        .with_lan_policy(
            vec![app_core::config::Network::parse("192.168.1.0/24").ok_or("net")?],
            vec!["accounts-pc".into()],
        );
    let app = App { _dir: dir, db, router: build_router(state, None) };
    let me = "/api/auth/me";
    let office = app.call_from("192.168.1.20:50000", "192.168.1.10:47800", Method::GET, me, None, None).await?;
    assert_eq!(office.status, StatusCode::UNAUTHORIZED, "reaches the API (then needs a login)");
    let named = app.call_from("192.168.1.20:50000", "ACCOUNTS-PC:47800", Method::GET, me, None, None).await?;
    assert_eq!(named.status, StatusCode::UNAUTHORIZED);
    let outside = app.call_from("8.8.8.8:50000", "192.168.1.10:47800", Method::GET, me, None, None).await?;
    assert_eq!(outside.status, StatusCode::FORBIDDEN, "not an office address");
    let other_subnet = app.call_from("192.168.2.20:50000", "192.168.1.10:47800", Method::GET, me, None, None).await?;
    assert_eq!(other_subnet.status, StatusCode::FORBIDDEN);
    let rebinding = app.call_from("127.0.0.1:50000", "evil.example:47800", Method::GET, me, None, None).await?;
    assert_eq!(rebinding.status, StatusCode::FORBIDDEN, "DNS rebinding through a foreign name");
    let local = app.call_from("127.0.0.1:50000", "localhost:47800", Method::GET, me, None, None).await?;
    assert_eq!(local.status, StatusCode::UNAUTHORIZED);
    Ok(())
}

// ── D17: first-run administrator ────────────────────────────────────────────

#[tokio::test]
async fn first_administrator_is_created_once_and_only_on_this_pc() -> TestResult {
    let app = App::new().await?;
    let path = "/api/auth/first-run";
    let status = app.call_from("127.0.0.1:50000", HOST, Method::GET, path, None, None).await?;
    assert_eq!(status.body, json!({"needsSetup": true, "canSetUpHere": true}));
    let remote_status = app.call_from("192.168.1.50:50000", HOST, Method::GET, path, None, None).await?;
    assert_ne!(remote_status.body["canSetUpHere"], json!(true));

    let good = json!({"username": "owner", "password": "a-long-password"});
    // Another PC (or an unknown origin) can never claim a fresh install.
    let remote = app.call_from("192.168.1.50:50000", HOST, Method::POST, path, None, Some(good.clone())).await?;
    assert_eq!(remote.status, StatusCode::FORBIDDEN);
    let no_peer = app.call(Method::POST, path, None, Some(good.clone())).await?;
    assert_eq!(no_peer.status, StatusCode::FORBIDDEN);
    assert!(!auth::users::any_users(&app.db).await?);

    let short = app
        .call_from(
            "127.0.0.1:50000",
            HOST,
            Method::POST,
            path,
            None,
            Some(json!({"username": "owner", "password": "short"})),
        )
        .await?;
    assert_eq!(short.status, StatusCode::BAD_REQUEST, "{}", short.body);

    let created = app.call_from("127.0.0.1:50000", HOST, Method::POST, path, None, Some(good.clone())).await?;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let token = created.body["accessToken"].as_str().ok_or("no accessToken")?.to_string();
    let me = app.call(Method::GET, "/api/auth/me", Some(&token), None).await?;
    assert_eq!(me.body["username"], json!("owner"));
    assert_eq!(me.body["role"]["name"], json!("Admin"));
    assert_eq!(me.body["isTwoFactorEnabled"], json!(false));

    let again = app
        .call_from(
            "127.0.0.1:50000",
            HOST,
            Method::POST,
            path,
            None,
            Some(json!({"username": "second", "password": "a-long-password"})),
        )
        .await?;
    assert_nest_error(&again, 403, json!("The app is already set up. Sign in instead."), path);
    let after = app.call_from("127.0.0.1:50000", HOST, Method::GET, path, None, None).await?;
    assert_eq!(after.body, json!({"needsSetup": false, "canSetUpHere": false}));

    let login = app.call(Method::POST, "/api/auth/login", None, Some(good)).await?;
    assert!(login.body["accessToken"].is_string(), "{}", login.body);
    let actions: Vec<String> =
        sqlx::query_scalar(r#"SELECT "action" FROM "audit_logs""#).fetch_all(app.db.read()).await?;
    assert!(actions.iter().any(|a| a == "Created the first administrator"), "{actions:?}");
    Ok(())
}

#[tokio::test]
async fn payments_are_voided_and_the_books_locked_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let clerk = app.signed_in("clerk", "Accountant").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (cash, revenue) = (id_of("1000")?, id_of("4000")?);
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Acme"}))).await?;
    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust.body["id"], "date": "2026-04-01",
                "lines": [{"description": "Design", "quantity": 1, "unitPrice": 300, "accountId": revenue}]})),
        )
        .await?;
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;
    // A part-payment, so the invoice still has a balance a later payment could go against.
    let payment = |date: &str| {
        json!({"type": "AR", "invoiceId": inv_id, "date": date, "amount": 100,
            "paymentMethod": "CASH", "paymentAccountId": cash})
    };
    let pay = app.call(Method::POST, "/api/payments", Some(&admin), Some(payment("2026-04-25"))).await?;
    assert_eq!((pay.status, pay.body["status"].as_str()), (StatusCode::CREATED, Some("POSTED")));
    let pay_id = pay.body["id"].as_str().ok_or("pay")?.to_string();

    // The payment's own entry cannot be reversed behind the invoice's back.
    let je = pay.body["journalEntryId"].as_str().ok_or("je")?;
    let sneaky = app.call(Method::POST, &format!("/api/ledger/entries/{je}/reverse"), Some(&admin), None).await?;
    assert_nest_error(
        &sneaky,
        400,
        json!("This entry was posted by a payment. Void or unpost it from that screen instead."),
        &format!("/api/ledger/entries/{je}/reverse"),
    );

    // Lock: anyone signed in can read it, only an administrator can move it.
    let open = app.call(Method::GET, "/api/books/lock", Some(&clerk), None).await?;
    assert_eq!(open.body, json!({"lockedThrough": null}));
    let lock = |d: Value| json!({ "lockedThrough": d });
    let denied = app.call(Method::PUT, "/api/books/lock", Some(&clerk), Some(lock(json!("2026-04-30")))).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    for bad in [json!("2026-13-01"), json!(20260430), json!("2999-01-01")] {
        let r = app.call(Method::PUT, "/api/books/lock", Some(&admin), Some(lock(bad.clone()))).await?;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}: {}", r.body);
    }
    let locked = app.call(Method::PUT, "/api/books/lock", Some(&admin), Some(lock(json!("2026-04-30")))).await?;
    assert_eq!(locked.body["lockedThrough"], "2026-04-30");
    let late = app.call(Method::POST, "/api/payments", Some(&admin), Some(payment("2026-04-30"))).await?;
    assert_eq!(late.status, StatusCode::BAD_REQUEST);
    assert!(late.body["message"]
        .as_str()
        .unwrap_or("")
        .starts_with("The books are locked up to and including 2026-04-30"));

    // Voiding is dated today, so it still works for a payment in the locked period.
    let void_path = format!("/api/payments/{pay_id}/void");
    let voided = app.call(Method::POST, &void_path, Some(&clerk), None).await?;
    assert_eq!((voided.status, voided.body["status"].as_str()), (StatusCode::CREATED, Some("VOID")), "{}", voided.body);
    let again = app.call(Method::POST, &void_path, Some(&clerk), None).await?;
    assert_nest_error(&again, 400, json!("Payment is already voided"), &void_path);
    let after = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!((after.body["status"].as_str(), after.body["amountPaid"].as_str()), (Some("POSTED"), Some("0.00")));
    let listed = app.call(Method::GET, "/api/payments", Some(&admin), None).await?;
    assert_eq!(listed.body["data"][0]["status"], "VOID", "a voided payment stays on the list");

    let unlocked = app.call(Method::PUT, "/api/books/lock", Some(&admin), Some(lock(Value::Null))).await?;
    assert_eq!(unlocked.body["lockedThrough"], Value::Null);

    let log = app.call(Method::GET, "/api/audit-logs", Some(&admin), None).await?;
    let data = log.body["data"].as_array().ok_or("data")?;
    let actions: Vec<(&str, &str)> =
        data.iter().map(|e| (e["action"].as_str().unwrap_or(""), e["detail"].as_str().unwrap_or(""))).collect();
    assert!(actions.contains(&("Changed the books lock date", "Locked through 2026-04-30")), "{actions:?}");
    assert!(actions.contains(&("Changed the books lock date", "Unlocked")), "{actions:?}");
    assert!(actions.iter().any(|(a, _)| *a == "Voided a payment"), "{actions:?}");
    Ok(())
}

#[tokio::test]
async fn a_fresh_install_gets_the_default_chart_once() -> TestResult {
    let app = App::new().await?;
    let codes = |db: &database::Db| {
        let db = db.clone();
        async move {
            let rows: Vec<String> = sqlx::query_scalar(r#"SELECT "code" FROM "accounts""#).fetch_all(db.read()).await?;
            Ok::<_, sqlx::Error>(rows)
        }
    };
    let before = codes(&app.db).await?;
    assert!(!before.contains(&"1000".to_string()), "the migration alone seeds only payroll accounts");

    let added = api_server::seed_fresh_install(&app.db).await?;
    assert!(added > 0);
    let after = codes(&app.db).await?;
    for code in ["1000", "1100", "2000", "3000", "4000", "6000"] {
        assert!(after.contains(&code.to_string()), "missing {code}");
    }
    assert_eq!(after.len(), after.iter().collect::<std::collections::BTreeSet<_>>().len(), "no duplicate codes");

    // Once someone uses the app, a deleted default account is not brought back.
    let admin = app.signed_in("admin", "Admin").await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let inventory = accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == "1200").ok_or("1200")?
        ["id"]
        .as_str()
        .ok_or("id")?
        .to_string();
    let deleted = app.call(Method::DELETE, &format!("/api/accounts/{inventory}"), Some(&admin), None).await?;
    assert!(deleted.status.is_success(), "{}", deleted.body);
    assert_eq!(api_server::seed_fresh_install(&app.db).await?, 0);
    assert!(!codes(&app.db).await?.contains(&"1200".to_string()));
    Ok(())
}

#[tokio::test]
async fn credit_and_debit_notes_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let viewer = app.signed_in("viewer", "Viewer").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let revenue = id_of("4000")?;
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Acme"}))).await?;
    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust.body["id"], "date": "2026-04-01",
                "lines": [{"description": "Design", "quantity": 1, "unitPrice": 500, "accountId": revenue}]})),
        )
        .await?;
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;

    let body = json!({"invoiceId": inv_id, "date": "2026-04-10", "reason": "Late delivery discount",
        "amount": 120.5, "accountId": revenue});
    let denied = app.call(Method::POST, "/api/credit-notes", Some(&viewer), Some(body.clone())).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "credit notes need the invoices permission");
    let invalid = app.call(Method::POST, "/api/credit-notes", Some(&admin), Some(json!({"invoiceId": inv_id}))).await?;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);

    let cn = app.call(Method::POST, "/api/credit-notes", Some(&admin), Some(body)).await?;
    assert_eq!(cn.status, StatusCode::CREATED, "{}", cn.body);
    assert_eq!((cn.body["noteNumber"].as_str(), cn.body["amount"].as_str()), (Some("CN-000001"), Some("120.50")));
    assert_eq!(cn.body["partyName"], "Acme");
    let after = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!(after.body["amountCredited"], "120.50");
    let aging = app.call(Method::GET, "/api/reports/ar-aging", Some(&admin), None).await?;
    assert_eq!(aging.body["grandTotal"], json!(379.5), "ageing shows only what is still owed");

    // The note's own entry is undone from the note, not the ledger screen.
    let je = cn.body["journalEntryId"].as_str().ok_or("je")?;
    let sneaky = app.call(Method::POST, &format!("/api/ledger/entries/{je}/reverse"), Some(&admin), None).await?;
    assert_eq!(sneaky.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        sneaky.body["message"],
        "This entry was posted by a credit note. Void or unpost it from that screen instead."
    );

    let cn_id = cn.body["id"].as_str().ok_or("cn")?;
    let voided = app.call(Method::POST, &format!("/api/credit-notes/{cn_id}/void"), Some(&admin), None).await?;
    assert_eq!((voided.status, voided.body["status"].as_str()), (StatusCode::CREATED, Some("VOID")));
    let list = app.call(Method::GET, "/api/credit-notes", Some(&admin), None).await?;
    assert_eq!(list.body.as_array().map(Vec::len), Some(1));
    // A debit-note route never finds a credit note.
    let wrong_kind = app.call(Method::POST, &format!("/api/debit-notes/{cn_id}/void"), Some(&admin), None).await?;
    assert_eq!(wrong_kind.status, StatusCode::NOT_FOUND);

    let log = app.call(Method::GET, "/api/audit-logs", Some(&admin), None).await?;
    let data = log.body["data"].as_array().ok_or("data")?;
    assert_eq!(data[0]["action"], "Voided a credit note");
    assert!(data.iter().any(|e| e["action"] == "Created a credit note" && e["detail"] == "CN-000001"));
    Ok(())
}

#[tokio::test]
async fn bank_reconciliation_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let clerk = app.signed_in("clerk", "Viewer").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (cash, equity, revenue) = (id_of("1000")?, id_of("3000")?, id_of("4000")?);
    let post = |date: &str, debit: &str, credit: &str, amount: f64| {
        json!({"date": date, "description": "test", "lines": [
            {"accountId": debit, "debit": amount, "credit": 0}, {"accountId": credit, "debit": 0, "credit": amount}]})
    };
    app.call(Method::POST, "/api/ledger/entries", Some(&admin), Some(post("2026-04-01", &cash, &equity, 1000.0)))
        .await?;
    app.call(Method::POST, "/api/ledger/entries", Some(&admin), Some(post("2026-04-15", &cash, &revenue, 200.25)))
        .await?;

    let start = json!({"accountId": cash, "statementDate": "2026-04-30", "statementBalance": 1000});
    // The Viewer role has `ledger`, so it may reconcile; an income account may not be reconciled.
    let not_bank = app
        .call(
            Method::POST,
            "/api/reconciliations",
            Some(&clerk),
            Some(json!({"accountId": revenue, "statementDate": "2026-04-30", "statementBalance": 0})),
        )
        .await?;
    assert_eq!(not_bank.status, StatusCode::BAD_REQUEST);
    let ws = app.call(Method::POST, "/api/reconciliations", Some(&clerk), Some(start)).await?;
    assert_eq!(ws.status, StatusCode::CREATED, "{}", ws.body);
    assert_eq!(ws.body["difference"], json!(1000));
    let id = ws.body["id"].as_str().ok_or("id")?.to_string();
    let lines = ws.body["lines"].as_array().ok_or("lines")?;
    assert_eq!(lines.len(), 2);
    let first = lines[0]["lineId"].as_str().ok_or("line")?.to_string();

    let ticked = app
        .call(Method::PUT, &format!("/api/reconciliations/{id}/lines"), Some(&clerk), Some(json!({"lineIds": [first]})))
        .await?;
    assert_eq!((ticked.body["clearedBalance"].clone(), ticked.body["difference"].clone()), (json!(1000), json!(0)));
    let bad = app
        .call(Method::PUT, &format!("/api/reconciliations/{id}/lines"), Some(&clerk), Some(json!({"lineIds": "x"})))
        .await?;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let done = app.call(Method::POST, &format!("/api/reconciliations/{id}/complete"), Some(&clerk), None).await?;
    assert_eq!((done.body["status"].as_str(), done.body["completedBy"].as_str()), (Some("COMPLETED"), Some("clerk")));

    let history = app.call(Method::GET, &format!("/api/reconciliations?accountId={cash}"), Some(&admin), None).await?;
    assert_eq!(history.body.as_array().map(Vec::len), Some(1));
    let undone = app.call(Method::POST, &format!("/api/reconciliations/{id}/undo"), Some(&admin), None).await?;
    assert_eq!(undone.body["status"], "DRAFT");
    let gone = app.call(Method::DELETE, &format!("/api/reconciliations/{id}"), Some(&admin), None).await?;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);

    let log = app.call(Method::GET, "/api/audit-logs", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("data")?.iter().filter_map(|e| e["action"].as_str()).collect();
    for want in [
        "Started a bank reconciliation",
        "Saved a bank reconciliation in progress",
        "Completed a bank reconciliation",
        "Undid a bank reconciliation",
        "Discarded a bank reconciliation",
    ] {
        assert!(actions.contains(&want), "{want} missing from {actions:?}");
    }
    Ok(())
}

#[tokio::test]
async fn statements_and_account_activity_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let viewer = app.signed_in("viewer", "Viewer").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let revenue = id_of("4000")?;
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Acme"}))).await?;
    let cust_id = cust.body["id"].as_str().ok_or("cust")?.to_string();
    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust_id, "date": "2026-04-01",
                "lines": [{"description": "Design", "quantity": 1, "unitPrice": 750, "accountId": revenue}]})),
        )
        .await?;
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;

    let path = format!("/api/customers/{cust_id}/statement?dateFrom=2026-01-01");
    let st = app.call(Method::GET, &path, Some(&admin), None).await?;
    assert_eq!(st.status, StatusCode::OK, "{}", st.body);
    assert_eq!((st.body["customer"]["name"].as_str(), st.body["closingBalance"].clone()), (Some("Acme"), json!(750)));
    assert_eq!(app.call(Method::GET, &path, Some(&viewer), None).await?.status, StatusCode::FORBIDDEN);
    let bad =
        app.call(Method::GET, &format!("/api/customers/{cust_id}/statement?dateFrom=nope"), Some(&admin), None).await?;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // The Viewer role has `ledger`, so it may drill into a report figure.
    let act = app
        .call(Method::GET, &format!("/api/ledger/activity/{revenue}?dateTo=2026-12-31"), Some(&viewer), None)
        .await?;
    assert_eq!(act.status, StatusCode::OK, "{}", act.body);
    assert_eq!((act.body["account"]["code"].as_str(), act.body["closingBalance"].clone()), (Some("4000"), json!(750)));
    assert_eq!(act.body["lines"][0]["sourceType"], "INVOICE");
    Ok(())
}

#[tokio::test]
async fn cash_flow_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let clerk = app.signed_in("clerk", "Accountant").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (cash, equity) = (id_of("1000")?, id_of("3000")?);
    let entry = json!({"date": "2026-04-01", "description": "Capital", "lines": [
        {"accountId": cash, "debit": 500, "credit": 0}, {"accountId": equity, "debit": 0, "credit": 500}]});
    app.call(Method::POST, "/api/ledger/entries", Some(&admin), Some(entry)).await?;

    // Without cashAccounts the 10xx accounts are cash.
    let cf = app
        .call(Method::GET, "/api/reports/cash-flow?dateFrom=2026-01-01&dateTo=2026-12-31", Some(&admin), None)
        .await?;
    assert_eq!(cf.status, StatusCode::OK, "{}", cf.body);
    assert_eq!(cf.body["cashAccounts"][0]["code"], "1000");
    assert_eq!((cf.body["financing"]["total"].clone(), cf.body["closingCash"].clone()), (json!(500), json!(500)));
    let bad = app.call(Method::GET, "/api/reports/cash-flow?cashAccounts=nope", Some(&admin), None).await?;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let revenue = id_of("4000")?;
    let not_cash =
        app.call(Method::GET, &format!("/api/reports/cash-flow?cashAccounts={revenue}"), Some(&admin), None).await?;
    assert_eq!(not_cash.status, StatusCode::BAD_REQUEST);
    let viewer_like = app.call(Method::GET, "/api/reports/cash-flow", Some(&clerk), None).await?;
    assert_eq!(viewer_like.status, StatusCode::OK, "the Accountant role has reports");
    Ok(())
}

#[tokio::test]
async fn company_and_region_settings_are_validated_and_drive_the_financial_year() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let fresh = app.call(Method::GET, "/api/settings", Some(&admin), None).await?;
    assert!(fresh.body.get("company.name").is_none(), "a fresh install has no company yet");

    let good = json!({
        "company.name": "  Lanka Traders  ", "company.address": "1 Main Street\nColombo", "company.taxNumber": "VAT-123",
        "region.country": "GB", "region.currency": "GBP", "region.locale": "en-GB", "region.fiscalYearStart": "4",
        "region.currencySymbol": "£", "region.currencyWord": "Pounds", "region.currencyMinorWord": "Pence",
        "company.logo": "data:image/png;base64,iVBORw0KGgo=",
    });
    let saved = app.call(Method::PATCH, "/api/settings", Some(&admin), Some(good)).await?;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
    assert_eq!(
        (saved.body["company.name"].as_str(), saved.body["region.currency"].as_str()),
        (Some("Lanka Traders"), Some("GBP"))
    );

    for (key, value) in [
        ("region.currency", "pounds"),
        ("region.country", "GBR"),
        ("region.locale", "English"),
        ("region.fiscalYearStart", "13"),
        ("company.logo", "/Logo_black.png"),
        ("company.logo", "data:text/html;base64,PHNjcmlwdD4="),
        ("company.name", "   "),
    ] {
        let r = app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({ key: value }))).await?;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{key}={value} must be refused");
    }

    // The P&L's default range starts on the first day of the financial year (April here).
    let pl = app.call(Method::GET, "/api/reports/profit-loss", Some(&admin), None).await?;
    let from = pl.body["dateFrom"].as_str().unwrap_or("").to_string();
    assert!(from.ends_with("-04-01"), "default dateFrom {from}");
    Ok(())
}

#[tokio::test]
async fn tax_codes_invoices_and_the_tax_return_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let clerk = app.signed_in("clerk", "Viewer").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let new_account = |code: &str, t: &str| json!({"code": code, "name": format!("Tax {code}"), "type": t});
    let payable = app.call(Method::POST, "/api/accounts", Some(&admin), Some(new_account("2300", "LIABILITY"))).await?;
    let receivable = app.call(Method::POST, "/api/accounts", Some(&admin), Some(new_account("1400", "ASSET"))).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (revenue, expense) = (id_of("4000")?, id_of("6000")?);

    let body = json!({"name": "GST 8.875%", "rate": 8.875,
        "salesAccountId": payable.body["id"], "purchaseAccountId": receivable.body["id"]});
    let denied = app.call(Method::POST, "/api/tax-codes", Some(&clerk), Some(body.clone())).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN, "managing tax codes needs settings");
    for bad in [json!(101), json!(-1), json!(8.12345), json!("18")] {
        let mut b = body.clone();
        b["rate"] = bad.clone();
        let r = app.call(Method::POST, "/api/tax-codes", Some(&admin), Some(b)).await?;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "rate {bad} must be refused");
    }
    let gst = app.call(Method::POST, "/api/tax-codes", Some(&admin), Some(body)).await?;
    assert_eq!(gst.status, StatusCode::CREATED, "{}", gst.body);
    assert_eq!(gst.body["rate"], json!(8.875));
    let gst_id = gst.body["id"].as_str().ok_or("gst")?.to_string();
    let listed = app.call(Method::GET, "/api/tax-codes", Some(&clerk), None).await?;
    assert_eq!(listed.body.as_array().map(Vec::len), Some(1), "anyone signed in can read them");

    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Acme"}))).await?;
    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust.body["id"], "date": "2026-04-01",
                "lines": [{"description": "Design", "quantity": 1, "unitPrice": 1000, "accountId": revenue, "taxCodeId": gst_id}]})),
        )
        .await?;
    assert_eq!(inv.status, StatusCode::CREATED, "{}", inv.body);
    assert_eq!((inv.body["totalAmount"].as_str(), inv.body["taxAmount"].as_str()), (Some("1088.75"), Some("88.75")));
    assert_eq!((inv.body["subtotal"].as_str(), inv.body["pricesIncludeTax"].clone()), (Some("1000.00"), json!(false)));
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();
    // Switching the draft to "prices include tax" recalculates the same lines.
    let inc = app
        .call(Method::PATCH, &format!("/api/invoices/{inv_id}"), Some(&admin), Some(json!({"pricesIncludeTax": true})))
        .await?;
    assert_eq!((inc.body["totalAmount"].clone(), inc.body["taxAmount"].as_str()), (json!(1000), Some("81.52")));
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;

    let vendor = app.call(Method::POST, "/api/vendors", Some(&admin), Some(json!({"name": "Paper Co"}))).await?;
    let bill = app
        .call(
            Method::POST,
            "/api/bills",
            Some(&admin),
            Some(json!({"vendorId": vendor.body["id"], "date": "2026-04-03",
                "lines": [{"description": "Paper", "quantity": 1, "unitPrice": 200, "accountId": expense, "taxCodeId": gst_id}]})),
        )
        .await?;
    assert_eq!(bill.body["taxAmount"].as_str(), Some("17.75"), "{}", bill.body);
    app.call(Method::POST, &format!("/api/bills/{}/post", bill.body["id"].as_str().ok_or("bill")?), Some(&admin), None)
        .await?;

    let ret =
        app.call(Method::GET, "/api/reports/tax?dateFrom=2026-04-01&dateTo=2026-04-30", Some(&admin), None).await?;
    assert_eq!(ret.status, StatusCode::OK, "{}", ret.body);
    let row = &ret.body["rows"][0];
    assert_eq!(
        (row["name"].as_str(), row["salesNet"].clone(), row["outputTax"].clone()),
        (Some("GST 8.875%"), json!(918.48), json!(81.52))
    );
    assert_eq!((row["purchasesNet"].clone(), row["inputTax"].clone()), (json!(200), json!(17.75)));
    // 1000 × 8.875/108.875 = 81.5155… → 81.52 (half-up); 81.52 − 17.75 = 63.77.
    assert_eq!(ret.body["netTax"], json!(63.77));
    let deleted = app.call(Method::DELETE, &format!("/api/tax-codes/{gst_id}"), Some(&admin), None).await?;
    assert_eq!(deleted.status, StatusCode::BAD_REQUEST, "a code in use is deactivated, not deleted");
    Ok(())
}

#[tokio::test]
async fn tax_groups_and_withholding_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let mut acct = std::collections::HashMap::new();
    for (code, name, t) in [
        ("2300", "Sales Tax Payable", "LIABILITY"),
        ("1400", "Tax Receivable", "ASSET"),
        ("1450", "WHT Receivable", "ASSET"),
        ("2350", "WHT Payable", "LIABILITY"),
    ] {
        let r = app
            .call(Method::POST, "/api/accounts", Some(&admin), Some(json!({"code": code, "name": name, "type": t})))
            .await?;
        acct.insert(code, r.body["id"].as_str().ok_or("acct")?.to_string());
    }
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (cash, revenue, expense) = (id_of("1000")?, id_of("4000")?, id_of("6000")?);
    let standard = |name: &str, rate: f64| json!({"name": name, "rate": rate, "salesAccountId": acct["2300"], "purchaseAccountId": acct["1400"]});
    let mut ids = Vec::new();
    for (name, rate) in [("State 6%", 6.0), ("County 1%", 1.0), ("Levy 10% on tax-inclusive", 10.0)] {
        let r = app.call(Method::POST, "/api/tax-codes", Some(&admin), Some(standard(name, rate))).await?;
        ids.push(r.body["id"].as_str().ok_or("code")?.to_string());
    }
    // State 6% + county 1% on the net, then a compound 10% on net + both.
    let group = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "Local sales tax", "kind": "GROUP", "components": [
                {"taxCodeId": ids[0]}, {"taxCodeId": ids[1]}, {"taxCodeId": ids[2], "isCompound": true}]})),
        )
        .await?;
    assert_eq!(group.status, StatusCode::CREATED, "{}", group.body);
    assert_eq!(group.body["rate"], json!(17.7), "6 + 1 + 10% of 107 = 17.7");
    let one = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "Bad", "kind": "GROUP", "components": [{"taxCodeId": ids[0]}]})),
        )
        .await?;
    assert_eq!(one.status, StatusCode::BAD_REQUEST, "a group needs two parts");
    let wht = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "WHT 5%", "kind": "WITHHOLDING", "rate": 5,
                "salesAccountId": acct["1450"], "purchaseAccountId": acct["2350"]})),
        )
        .await?;
    assert_eq!(wht.status, StatusCode::CREATED, "{}", wht.body);
    let wht_id = wht.body["id"].as_str().ok_or("wht")?.to_string();

    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Acme"}))).await?;
    let mut line = json!({"description": "Goods", "quantity": 1, "unitPrice": 1000, "accountId": revenue,
        "taxCodeId": group.body["id"]});
    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust.body["id"], "date": "2026-04-01", "lines": [line.clone()]})),
        )
        .await?;
    assert_eq!(
        (inv.body["taxAmount"].as_str(), inv.body["totalAmount"].as_str()),
        (Some("177.00"), Some("1177.00")),
        "{}",
        inv.body
    );
    let parts: Vec<&str> = inv.body["lines"][0]["taxes"]
        .as_array()
        .ok_or("taxes")?
        .iter()
        .filter_map(|t| t["taxAmount"].as_str())
        .collect();
    assert_eq!(parts, vec!["60.00", "10.00", "107.00"]);
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;
    // A withholding code is never charged on a line.
    line["taxCodeId"] = json!(wht_id);
    let wrong = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust.body["id"], "date": "2026-04-01", "lines": [line]})),
        )
        .await?;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);

    // The customer pays 1,127.00 and withholds 50.00: together they settle the invoice.
    let pay = |amount: f64, withheld: f64| {
        json!({"type": "AR", "invoiceId": inv_id, "date": "2026-04-20", "amount": amount, "paymentMethod": "BANK_TRANSFER",
            "paymentAccountId": cash, "withholdingTaxCodeId": wht_id, "withholdingAmount": withheld})
    };
    let too_much = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay(1127.0, 50.01))).await?;
    assert_eq!(too_much.status, StatusCode::BAD_REQUEST, "money plus withholding cannot exceed the balance");
    let paid = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay(1127.0, 50.0))).await?;
    assert_eq!(paid.status, StatusCode::CREATED, "{}", paid.body);
    let after = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!(after.body["status"], "PAID");

    // We pay a vendor 950.00 of a 1,000.00 bill and withhold 50.00 for the tax office.
    let vendor = app.call(Method::POST, "/api/vendors", Some(&admin), Some(json!({"name": "Consultant"}))).await?;
    let bill = app
        .call(
            Method::POST,
            "/api/bills",
            Some(&admin),
            Some(json!({"vendorId": vendor.body["id"], "date": "2026-04-05",
                "lines": [{"description": "Advice", "quantity": 1, "unitPrice": 1000, "accountId": expense}]})),
        )
        .await?;
    let bill_id = bill.body["id"].as_str().ok_or("bill")?.to_string();
    app.call(Method::POST, &format!("/api/bills/{bill_id}/post"), Some(&admin), None).await?;
    let vendor_pay = json!({"type": "AP", "billId": bill_id, "date": "2026-04-25", "amount": 950, "paymentMethod": "BANK_TRANSFER",
        "paymentAccountId": cash, "withholdingTaxCodeId": wht_id, "withholdingAmount": 50});
    let vp = app.call(Method::POST, "/api/payments", Some(&admin), Some(vendor_pay)).await?;
    assert_eq!(vp.status, StatusCode::CREATED, "{}", vp.body);

    let tb = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    let bal = |code: &str| {
        tb.body
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["account"]["code"] == code).map(|r| r["balance"].clone()))
    };
    assert_eq!(bal("2300"), Some(json!(177)), "all three parts of the group post to the sales tax account");
    assert_eq!((bal("1450"), bal("2350")), (Some(json!(50)), Some(json!(50))));
    assert_eq!(bal("1100"), Some(json!(0)));
    assert_eq!(bal("2000"), Some(json!(0)));

    let ret =
        app.call(Method::GET, "/api/reports/tax?dateFrom=2026-04-01&dateTo=2026-04-30", Some(&admin), None).await?;
    let rows = ret.body["rows"].as_array().ok_or("rows")?;
    let row =
        |name: &str| rows.iter().find(|r| r["name"] == name).map(|r| (r["salesNet"].clone(), r["outputTax"].clone()));
    assert_eq!(row("State 6%"), Some((json!(1000), json!(60))));
    assert_eq!(row("Levy 10% on tax-inclusive"), Some((json!(1000), json!(107))));
    assert_eq!(ret.body["totals"]["salesNet"], json!(1000), "a line under three parts is one sale");
    assert_eq!(ret.body["netTax"], json!(177));
    assert_eq!(
        (
            ret.body["withholdingTotals"]["withheldByCustomers"].clone(),
            ret.body["withholdingTotals"]["withheldFromVendors"].clone()
        ),
        (json!(50), json!(50))
    );
    // Voiding the payment puts the money and the withholding back on the invoice.
    let pay_id = paid.body["id"].as_str().ok_or("pay")?;
    app.call(Method::POST, &format!("/api/payments/{pay_id}/void"), Some(&admin), None).await?;
    let reopened = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!(
        (reopened.body["status"].as_str(), reopened.body["amountPaid"].as_str()),
        (Some("POSTED"), Some("0.00"))
    );
    Ok(())
}

#[tokio::test]
async fn currencies_with_three_decimals_are_refused() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let r = app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"region.currency": "KWD"}))).await?;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        r.body["message"][0],
        "KWD has three decimal places, which the app cannot keep yet — choose another currency"
    );
    Ok(())
}

#[tokio::test]
async fn foreign_currency_invoices_bills_payments_and_exchange_differences() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(
        Method::PATCH,
        "/api/settings",
        Some(&admin),
        Some(json!({"company.name": "Lanka Ltd", "region.currency": "LKR"})),
    )
    .await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let vat_acct = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(json!({"code": "2300", "name": "VAT Payable", "type": "LIABILITY"})),
        )
        .await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let id_of = |code: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(accounts.body.as_array().ok_or("accounts")?.iter().find(|a| a["code"] == code).ok_or("code")?["id"]
            .as_str()
            .ok_or("id")?
            .to_string())
    };
    let (cash, revenue, expense) = (id_of("1000")?, id_of("4000")?, id_of("6000")?);
    let vat = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "VAT 10%", "rate": 10, "salesAccountId": vat_acct.body["id"], "purchaseAccountId": vat_acct.body["id"]})),
        )
        .await?;
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "US Client"}))).await?;
    let invoice = |amount: f64, tax: bool| {
        let mut line = json!({"description": "Services", "quantity": 1, "unitPrice": amount, "accountId": revenue});
        if tax {
            line["taxCodeId"] = vat.body["id"].clone();
        }
        json!({"customerId": cust.body["id"], "date": "2026-04-01", "currency": "usd", "exchangeRate": 300, "lines": [line]})
    };
    let no_rate = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": cust.body["id"], "date": "2026-04-01", "currency": "USD",
                "lines": [{"description": "x", "quantity": 1, "unitPrice": 1, "accountId": revenue}]})),
        )
        .await?;
    assert_eq!(no_rate.status, StatusCode::BAD_REQUEST, "a foreign invoice needs its rate");

    // USD 100 + VAT 10% at 300 → USD 110 = LKR 33,000 (revenue 30,000, VAT 3,000).
    let inv = app.call(Method::POST, "/api/invoices", Some(&admin), Some(invoice(100.0, true))).await?;
    assert_eq!(inv.status, StatusCode::CREATED, "{}", inv.body);
    assert_eq!((inv.body["currency"].as_str(), inv.body["totalAmount"].as_str()), (Some("USD"), Some("110.00")));
    assert_eq!(inv.body["baseTotal"].as_str(), Some("33000.00"));
    let inv_id = inv.body["id"].as_str().ok_or("inv")?.to_string();
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;
    let tb = |app_body: &Value, code: &str| {
        app_body
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["account"]["code"] == code).map(|r| r["balance"].clone()))
    };
    let t1 = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(
        (tb(&t1.body, "1100"), tb(&t1.body, "4000"), tb(&t1.body, "2300")),
        (Some(json!(33000)), Some(json!(30000)), Some(json!(3000)))
    );

    // Paid USD 60 at 310 (LKR 18,600) for USD 60 booked at 300 (18,000): a gain of 600.
    let pay = |doc: &str, amount: f64, rate: f64| {
        json!({"type": "AR", "invoiceId": doc, "date": "2026-04-20", "amount": amount, "exchangeRate": rate,
            "paymentMethod": "BANK_TRANSFER", "paymentAccountId": cash})
    };
    let p1 = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay(&inv_id, 60.0, 310.0))).await?;
    assert_eq!(p1.status, StatusCode::CREATED, "{}", p1.body);
    assert_eq!(
        (p1.body["baseAmount"].as_str(), p1.body["baseSettled"].as_str(), p1.body["exchangeDifference"].as_str()),
        (Some("18600.00"), Some("18000.00"), Some("-600.00"))
    );
    // The rest, USD 50 at 290 (14,500), clears what is left as booked (15,000): a loss of 500.
    let p2 = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay(&inv_id, 50.0, 290.0))).await?;
    assert_eq!(
        (p2.body["baseSettled"].as_str(), p2.body["exchangeDifference"].as_str()),
        (Some("15000.00"), Some("500.00"))
    );
    let paid = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!((paid.body["status"].as_str(), paid.body["baseAmountPaid"].as_str()), (Some("PAID"), Some("33000.00")));

    // A credit note on a USD invoice uses the invoice's rate: no exchange difference.
    let inv2 = app.call(Method::POST, "/api/invoices", Some(&admin), Some(invoice(50.0, false))).await?;
    let inv2_id = inv2.body["id"].as_str().ok_or("inv2")?.to_string();
    app.call(Method::POST, &format!("/api/invoices/{inv2_id}/post"), Some(&admin), None).await?;
    let cn = app
        .call(
            Method::POST,
            "/api/credit-notes",
            Some(&admin),
            Some(json!({"invoiceId": inv2_id, "date": "2026-04-10", "reason": "Discount", "amount": 20, "accountId": revenue})),
        )
        .await?;
    assert_eq!(cn.status, StatusCode::CREATED, "{}", cn.body);
    let aging = app.call(Method::GET, "/api/reports/ar-aging", Some(&admin), None).await?;
    assert_eq!(aging.body["grandTotal"], json!(9000), "USD 30 still owed, booked at 300");
    let p3 = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay(&inv2_id, 30.0, 300.0))).await?;
    assert_eq!(p3.body["exchangeDifference"].as_str(), Some("0.00"));

    // A EUR bill: 10 at 330 (3,300) paid at 320 (3,200) — we paid less: a gain of 100.
    let vendor = app.call(Method::POST, "/api/vendors", Some(&admin), Some(json!({"name": "EU Supplier"}))).await?;
    let bill = app
        .call(
            Method::POST,
            "/api/bills",
            Some(&admin),
            Some(json!({"vendorId": vendor.body["id"], "date": "2026-04-05", "currency": "EUR", "exchangeRate": 330,
                "lines": [{"description": "Parts", "quantity": 1, "unitPrice": 10, "accountId": expense}]})),
        )
        .await?;
    assert_eq!(bill.body["baseTotal"].as_str(), Some("3300.00"), "{}", bill.body);
    let bill_id = bill.body["id"].as_str().ok_or("bill")?.to_string();
    app.call(Method::POST, &format!("/api/bills/{bill_id}/post"), Some(&admin), None).await?;
    let bp = app
        .call(
            Method::POST,
            "/api/payments",
            Some(&admin),
            Some(json!({"type": "AP", "billId": bill_id, "date": "2026-04-25", "amount": 10, "exchangeRate": 320,
                "paymentMethod": "BANK_TRANSFER", "paymentAccountId": cash})),
        )
        .await?;
    assert_eq!(bp.body["exchangeDifference"].as_str(), Some("100.00"), "{}", bp.body);

    let t2 = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t2.body, "1100"), Some(json!(0)), "every USD receivable cleared at its booked amount");
    assert_eq!(tb(&t2.body, "2000"), Some(json!(0)));
    // Exchange account (an expense): +500 loss − 600 gain − 100 gain = −200, a net gain of 200.
    assert_eq!(tb(&t2.body, "7900"), Some(json!(-200)));
    // Cash: 18,600 + 14,500 + 9,000 − 3,200.
    assert_eq!(tb(&t2.body, "1000"), Some(json!(38900)));
    let rates = app.call(Method::GET, "/api/settings", Some(&admin), None).await?;
    assert_eq!((rates.body["fx.rate.USD"].as_str(), rates.body["fx.rate.EUR"].as_str()), (Some("300"), Some("320")));
    Ok(())
}

#[tokio::test]
async fn payable_accounts_account_edits_and_a_usd_bank_account() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(
        Method::PATCH,
        "/api/settings",
        Some(&admin),
        Some(json!({"company.name": "Lanka Ltd", "region.currency": "LKR"})),
    )
    .await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let new_account = |code: &str, name: &str, kind: &str, currency: Option<&str>| {
        let mut body = json!({"code": code, "name": name, "type": kind});
        if let Some(c) = currency {
            body["currency"] = json!(c);
        }
        body
    };
    let tel = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(new_account("2400", "Telephone Payable", "LIABILITY", None)),
        )
        .await?;
    let tel_exp = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(new_account("6500", "Telephone Expense", "EXPENSE", None)),
        )
        .await?;
    let capital = app
        .call(Method::POST, "/api/accounts", Some(&admin), Some(new_account("3100", "Capital", "EQUITY", None)))
        .await?;
    let usd = app
        .call(Method::POST, "/api/accounts", Some(&admin), Some(new_account("1020", "USD Bank", "ASSET", Some("usd"))))
        .await?;
    assert_eq!(usd.body["currency"].as_str(), Some("USD"), "{}", usd.body);
    let lkr_home = app
        .call(Method::POST, "/api/accounts", Some(&admin), Some(new_account("1030", "Odd", "ASSET", Some("LKR"))))
        .await?;
    assert!(lkr_home.body["currency"].is_null(), "the home currency is stored as none");
    let id = |v: &Value| v["id"].as_str().unwrap_or_default().to_string();
    let (tel, tel_exp, capital, usd) = (id(&tel.body), id(&tel_exp.body), id(&capital.body), id(&usd.body));
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let code_id = |code: &str| -> String {
        accounts.body.as_array().and_then(|a| a.iter().find(|x| x["code"] == code)).map(id).unwrap_or_default()
    };
    let (cash, revenue) = (code_id("1000"), code_id("4000"));
    let tb = |body: &Value, code: &str| {
        body.as_array()
            .and_then(|rows| rows.iter().find(|r| r["account"]["code"] == code).map(|r| r["balance"].clone()))
    };

    // The senior's example: a telephone bill accrued by journal entry, owed on its own payable.
    let je = app
        .call(
            Method::POST,
            "/api/ledger/entries",
            Some(&admin),
            Some(json!({"date": "2026-04-30", "description": "Telephone bill April", "lines": [
                {"accountId": tel_exp, "debit": 18337.13, "credit": 0},
                {"accountId": tel, "debit": 0, "credit": 18337.13}]})),
        )
        .await?;
    assert_eq!(je.status, StatusCode::CREATED, "{}", je.body);

    // A bill owed on Telephone Payable instead of 2000, with a debit note, then paid.
    let vendor = app.call(Method::POST, "/api/vendors", Some(&admin), Some(json!({"name": "Telecom"}))).await?;
    let bad = app
        .call(
            Method::POST,
            "/api/bills",
            Some(&admin),
            Some(json!({"vendorId": vendor.body["id"], "date": "2026-05-01", "payableAccountId": tel_exp,
            "lines": [{"description": "May", "quantity": 1, "unitPrice": 100, "accountId": tel_exp}]})),
        )
        .await?;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "an expense is not a payable account");
    let bill = app
        .call(
            Method::POST,
            "/api/bills",
            Some(&admin),
            Some(json!({"vendorId": vendor.body["id"], "date": "2026-05-01", "payableAccountId": tel,
            "lines": [{"description": "Telephone May", "quantity": 1, "unitPrice": 20000, "accountId": tel_exp}]})),
        )
        .await?;
    assert_eq!(bill.body["payableAccountId"].as_str(), Some(tel.as_str()), "{}", bill.body);
    let bill_id = id(&bill.body);
    app.call(Method::POST, &format!("/api/bills/{bill_id}/post"), Some(&admin), None).await?;
    let dn = app
        .call(Method::POST, "/api/debit-notes", Some(&admin), Some(json!({"billId": bill_id, "date": "2026-05-03", "reason": "Overcharge", "amount": 2000, "accountId": tel_exp})))
        .await?;
    assert_eq!(dn.status, StatusCode::CREATED, "{}", dn.body);
    let t1 = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t1.body, "2400"), Some(json!(36337.13)), "18,337.13 accrued + 20,000 billed - 2,000 debit note");
    assert!(tb(&t1.body, "2000").is_none_or(|v| v == json!(0)), "nothing went to Accounts Payable");
    let pay_bill = app
        .call(
            Method::POST,
            "/api/payments",
            Some(&admin),
            Some(json!({"type": "AP", "billId": bill_id, "date": "2026-05-20", "amount": 18000,
            "paymentMethod": "BANK_TRANSFER", "paymentAccountId": cash})),
        )
        .await?;
    assert_eq!(pay_bill.status, StatusCode::CREATED, "{}", pay_bill.body);

    // Paying the payable account itself, with no bill behind it.
    let pay_acct = |amount: f64, account: &str, from: &str, rate: Option<f64>| {
        let mut b = json!({"type": "AP", "payableAccountId": account, "date": "2026-05-25", "amount": amount,
            "paymentMethod": "BANK_TRANSFER", "paymentAccountId": from});
        if let Some(r) = rate {
            b["exchangeRate"] = json!(r);
        }
        b
    };
    let wrong =
        app.call(Method::POST, "/api/payments", Some(&admin), Some(pay_acct(10.0, &revenue, &cash, None))).await?;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST, "revenue is not a payable account");
    let p = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay_acct(18337.13, &tel, &cash, None))).await?;
    assert_eq!(p.status, StatusCode::CREATED, "{}", p.body);
    assert_eq!(p.body["payableAccountId"].as_str(), Some(tel.as_str()));
    let t2 = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t2.body, "2400"), Some(json!(0)), "the telephone payable is cleared");
    // Voiding it puts the liability back.
    let void = app.call(Method::POST, &format!("/api/payments/{}/void", id(&p.body)), Some(&admin), None).await?;
    assert!(void.status.is_success(), "{}", void.body);
    let t3 = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t3.body, "2400"), Some(json!(18337.13)));

    // Editing the chart: a name changes; a used account keeps its type.
    let renamed = app
        .call(
            Method::PATCH,
            &format!("/api/accounts/{tel}"),
            Some(&admin),
            Some(json!({"name": "Telephone & Internet Payable", "subtype": "current_liability"})),
        )
        .await?;
    assert_eq!(renamed.body["name"].as_str(), Some("Telephone & Internet Payable"), "{}", renamed.body);
    let retype = app
        .call(Method::PATCH, &format!("/api/accounts/{tel}"), Some(&admin), Some(json!({"type": "EXPENSE"})))
        .await?;
    assert_eq!(retype.status, StatusCode::BAD_REQUEST, "a used account keeps its type");

    // The USD bank account: every line on it needs its USD amount.
    let no_usd = app
        .call(
            Method::POST,
            "/api/ledger/entries",
            Some(&admin),
            Some(json!({"date": "2026-04-01", "description": "USD deposit", "lines": [
            {"accountId": usd, "debit": 300000, "credit": 0}, {"accountId": capital, "debit": 0, "credit": 300000}]})),
        )
        .await?;
    assert_eq!(no_usd.status, StatusCode::BAD_REQUEST, "a USD account line needs its USD amount");
    let deposit = app
        .call(Method::POST, "/api/ledger/entries", Some(&admin), Some(json!({"date": "2026-04-01", "description": "USD deposit", "lines": [
            {"accountId": usd, "debit": 300000, "credit": 0, "foreignAmount": 1000}, {"accountId": capital, "debit": 0, "credit": 300000}]})))
        .await?;
    assert_eq!(deposit.status, StatusCode::CREATED, "{}", deposit.body);
    // A USD invoice paid into the USD account; an LKR one cannot be.
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "US Client"}))).await?;
    let inv = |cur: Option<&str>| {
        let mut b = json!({"customerId": cust.body["id"], "date": "2026-04-05",
            "lines": [{"description": "Export", "quantity": 1, "unitPrice": 200, "accountId": revenue}]});
        if let Some(c) = cur {
            b["currency"] = json!(c);
            b["exchangeRate"] = json!(300);
        }
        b
    };
    let usd_inv = app.call(Method::POST, "/api/invoices", Some(&admin), Some(inv(Some("USD")))).await?;
    let lkr_inv = app.call(Method::POST, "/api/invoices", Some(&admin), Some(inv(None))).await?;
    for i in [&usd_inv, &lkr_inv] {
        app.call(Method::POST, &format!("/api/invoices/{}/post", id(&i.body)), Some(&admin), None).await?;
    }
    let receive = |inv: &Value, rate: f64| {
        json!({"type": "AR", "invoiceId": inv["id"], "date": "2026-04-20", "amount": 200, "exchangeRate": rate,
            "paymentMethod": "BANK_TRANSFER", "paymentAccountId": usd})
    };
    let refused = app.call(Method::POST, "/api/payments", Some(&admin), Some(receive(&lkr_inv.body, 1.0))).await?;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "an LKR invoice is not paid into the USD account");
    let got = app.call(Method::POST, "/api/payments", Some(&admin), Some(receive(&usd_inv.body, 305.0))).await?;
    assert_eq!(got.status, StatusCode::CREATED, "{}", got.body);
    // Paying the telephone payable from the USD account at 310: USD 50 = LKR 15,500.
    let no_rate = app.call(Method::POST, "/api/payments", Some(&admin), Some(pay_acct(50.0, &tel, &usd, None))).await?;
    assert_eq!(no_rate.status, StatusCode::BAD_REQUEST, "paying from USD needs the rate");
    let usd_out =
        app.call(Method::POST, "/api/payments", Some(&admin), Some(pay_acct(50.0, &tel, &usd, Some(310.0)))).await?;
    assert_eq!(usd_out.body["baseAmount"].as_str(), Some("15500.00"), "{}", usd_out.body);

    // USD balance 1,000 + 200 - 50 = 1,150; LKR 300,000 + 61,000 - 15,500 = 345,500.
    let act = app.call(Method::GET, &format!("/api/ledger/activity/{usd}"), Some(&admin), None).await?;
    assert_eq!(
        (act.body["currency"].as_str(), act.body["foreignClosing"].clone(), act.body["closingBalance"].clone()),
        (Some("USD"), json!(1150), json!(345500)),
        "{}",
        act.body
    );
    // Revalued at 302 on 31 May: worth 347,300, a gain of 1,800.
    let reval = app
        .call(
            Method::POST,
            &format!("/api/accounts/{usd}/revalue"),
            Some(&admin),
            Some(json!({"date": "2026-05-31", "exchangeRate": 302})),
        )
        .await?;
    assert_eq!(reval.status, StatusCode::CREATED, "{}", reval.body);
    let t4 = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t4.body, "1020"), Some(json!(347300)));
    assert_eq!(tb(&t4.body, "7900"), Some(json!(-2800)), "realised gain 1,000 on the receipt + unrealised 1,800");
    let again = app
        .call(
            Method::POST,
            &format!("/api/accounts/{usd}/revalue"),
            Some(&admin),
            Some(json!({"date": "2026-05-31", "exchangeRate": 302})),
        )
        .await?;
    assert_eq!(again.status, StatusCode::NO_CONTENT, "nothing left to revalue");
    let recurrency =
        app.call(Method::PATCH, &format!("/api/accounts/{usd}"), Some(&admin), Some(json!({"currency": ""}))).await?;
    assert_eq!(recurrency.status, StatusCode::BAD_REQUEST, "a used account keeps its currency");
    // Reversing the deposit brings the USD balance back exactly.
    let rev = app
        .call(Method::POST, &format!("/api/ledger/entries/{}/reverse", id(&deposit.body)), Some(&admin), None)
        .await?;
    assert!(rev.status.is_success(), "{}", rev.body);
    let act2 = app.call(Method::GET, &format!("/api/ledger/activity/{usd}"), Some(&admin), None).await?;
    assert_eq!(act2.body["foreignClosing"], json!(150));
    Ok(())
}
