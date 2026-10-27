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

#[tokio::test]
async fn admins_cannot_lock_themselves_out_and_the_log_filters() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    let _clerk = app.signed_in("clerk", "Viewer").await?;
    let users = app.call(Method::GET, "/api/users", Some(&admin), None).await?;
    let id_of = |name: &str| -> String {
        users
            .body
            .as_array()
            .and_then(|a| a.iter().find(|u| u["username"] == name))
            .and_then(|u| u["id"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let (me, clerk) = (id_of("admin"), id_of("clerk"));
    let roles = app.call(Method::GET, "/api/roles", Some(&admin), None).await?;
    let role = |name: &str| -> String {
        roles
            .body
            .as_array()
            .and_then(|a| a.iter().find(|r| r["name"] == name))
            .and_then(|r| r["id"].as_str())
            .unwrap_or_default()
            .to_string()
    };

    // Nobody deactivates themselves or drops their own Admin role.
    let off =
        app.call(Method::PATCH, &format!("/api/users/{me}"), Some(&admin), Some(json!({"isActive": false}))).await?;
    assert_eq!(off.status, StatusCode::BAD_REQUEST, "{}", off.body);
    let demote = app
        .call(Method::PATCH, &format!("/api/users/{me}"), Some(&admin), Some(json!({"roleId": role("Viewer")})))
        .await?;
    assert_eq!(demote.status, StatusCode::BAD_REQUEST, "{}", demote.body);
    // Another user can be changed freely.
    let clerk_off =
        app.call(Method::PATCH, &format!("/api/users/{clerk}"), Some(&admin), Some(json!({"isActive": false}))).await?;
    assert_eq!(clerk_off.status, StatusCode::OK, "{}", clerk_off.body);
    // With a second administrator, the first one can be demoted by that second one.
    app.call(
        Method::PATCH,
        &format!("/api/users/{clerk}"),
        Some(&admin),
        Some(json!({"isActive": true, "roleId": role("Admin")})),
    )
    .await?;
    let ok = app
        .call(Method::PATCH, &format!("/api/users/{me}"), Some(&admin), Some(json!({"roleId": role("Viewer")})))
        .await?;
    assert_eq!(ok.status, StatusCode::BAD_REQUEST, "still not by themselves");

    // Last sign-in comes from the log.
    let last = app.call(Method::GET, "/api/users/last-sign-in", Some(&admin), None).await?;
    assert!(last.body.as_array().is_some_and(|a| a.iter().any(|x| x["userId"] == me.as_str())), "{}", last.body);

    // The log narrows by kind and by text.
    let signins = app.call(Method::GET, "/api/audit-logs?kind=signin&limit=100", Some(&admin), None).await?;
    let rows = signins.body["data"].as_array().cloned().unwrap_or_default();
    assert!(
        !rows.is_empty()
            && rows
                .iter()
                .all(|r| r["action"].as_str().is_some_and(|a| a.contains("Logged in") || a.contains("login"))),
        "{}",
        signins.body
    );
    let users_kind = app.call(Method::GET, "/api/audit-logs?kind=users&limit=100", Some(&admin), None).await?;
    assert!(users_kind.body["data"]
        .as_array()
        .is_some_and(|a| a.iter().all(|r| !r["action"].as_str().unwrap_or("").contains("Logged in"))));
    let search = app.call(Method::GET, "/api/audit-logs?search=CLERK&limit=100", Some(&admin), None).await?;
    assert!(search.body["total"].as_i64().unwrap_or(0) >= 1, "{}", search.body);
    let future = app.call(Method::GET, "/api/audit-logs?dateFrom=2999-01-01", Some(&admin), None).await?;
    assert_eq!(future.body["total"], json!(0));
    Ok(())
}

#[tokio::test]
async fn import_from_file_bank_matching_and_undo() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"region.currency": "LKR"}))).await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let code_id = |code: &str| -> String {
        accounts
            .body
            .as_array()
            .and_then(|a| a.iter().find(|x| x["code"] == code))
            .and_then(|x| x["id"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let (cash, revenue, opex) = (code_id("1000"), code_id("4000"), code_id("6000"));
    let id = |v: &Value| v["id"].as_str().unwrap_or_default().to_string();

    // Reading a file: a bank's CSV with a BOM, semicolons and a quoted field.
    let csv = "\u{feff}Date;Details;Amount\n05/10/2026;\"Deposit; ABC Traders\";1,000.00\n".as_bytes();
    let read =
        app.upload("/api/imports/read", &admin, multipart(&[], Some(("file", "stmt.csv", "text/csv", csv)))).await?;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(read.body["sheets"][0]["rows"][1], json!(["05/10/2026", "Deposit; ABC Traders", "1,000.00"]));
    let pdf = app
        .upload("/api/imports/read", &admin, multipart(&[], Some(("file", "a.pdf", "application/pdf", b"%PDF"))))
        .await?;
    assert_eq!(pdf.status, StatusCode::BAD_REQUEST);

    // An invoice for ABC Traders, posted, to be paid from the statement.
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "ABC Traders"}))).await?;
    let inv = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": id(&cust.body), "date": "2026-10-01",
                "lines": [{"description": "Goods", "quantity": 1, "unitPrice": 1000, "accountId": revenue}]})),
        )
        .await?;
    let inv_id = id(&inv.body);
    app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;

    let bank = json!({"kind": "BANK", "source": "Commercial Bank", "fileName": "stmt.csv", "accountId": cash, "dateOrder": "DMY",
    "rows": [
        {"date": "05/10/2026", "description": "Deposit ABC Traders", "amount": "1,000.00"},
        {"date": "06/10/2026", "description": "Bank charges", "moneyIn": "", "moneyOut": "250.00"},
        {"date": "31/02/2026", "description": "Bad date", "amount": "1"},
        {"date": "07/10/2026", "description": "Opening balance", "amount": ""}
    ]});
    // The preview saves nothing and is not logged.
    let preview = app.call(Method::POST, "/api/imports/preview", Some(&admin), Some(bank.clone())).await?;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    assert_eq!((preview.body["created"].clone(), preview.body["errors"].clone()), (json!(2), json!(2)));
    assert!(preview.body["id"].is_null());
    let statuses: Vec<&str> =
        preview.body["rows"].as_array().ok_or("rows")?.iter().filter_map(|r| r["status"].as_str()).collect();
    assert_eq!(statuses, ["NEW", "NEW", "ERROR", "ERROR"]);
    let lines_url = format!("/api/bank-lines?accountId={cash}&status=UNMATCHED");
    let none = app.call(Method::GET, &lines_url, Some(&admin), None).await?;
    assert_eq!(none.body["data"], json!([]), "a preview leaves nothing behind");

    let imported = app.call(Method::POST, "/api/imports", Some(&admin), Some(bank.clone())).await?;
    assert_eq!(imported.status, StatusCode::CREATED, "{}", imported.body);
    assert_eq!(imported.body["created"], json!(2));
    let import_id = imported.body["id"].as_str().ok_or("import id")?.to_string();
    // The same file again: nothing new.
    let again = app.call(Method::POST, "/api/imports", Some(&admin), Some(bank.clone())).await?;
    assert_eq!((again.body["created"].clone(), again.body["skipped"].clone()), (json!(0), json!(2)));
    assert!(again.body["id"].is_null(), "an import with nothing new is not kept");

    // The deposit is suggested against the open invoice; the payment is recorded from the line.
    let lines = app.call(Method::GET, &lines_url, Some(&admin), None).await?;
    let data = lines.body["data"].as_array().ok_or("lines")?.clone();
    assert_eq!(data.len(), 2, "{}", lines.body);
    let deposit = data.iter().find(|l| l["amount"] == json!(1000)).ok_or("deposit")?;
    let charges = data.iter().find(|l| l["amount"] == json!(-250)).ok_or("charges")?;
    assert_eq!(deposit["suggestions"][0]["type"], json!("INVOICE"), "{deposit}");
    assert_eq!(deposit["suggestions"][0]["id"], json!(inv_id));
    let wrong = app
        .call(
            Method::POST,
            &format!("/api/bank-lines/{}/payment", id(charges)),
            Some(&admin),
            Some(json!({"invoiceId": inv_id})),
        )
        .await?;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST, "money out is not paid against an invoice");
    let paid = app
        .call(
            Method::POST,
            &format!("/api/bank-lines/{}/payment", id(deposit)),
            Some(&admin),
            Some(json!({"invoiceId": inv_id})),
        )
        .await?;
    assert_eq!(paid.status, StatusCode::OK, "{}", paid.body);
    assert_eq!((paid.body["status"].as_str(), paid.body["match"]["type"].as_str()), (Some("MATCHED"), Some("PAYMENT")));
    let inv_after = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    assert_eq!(inv_after.body["status"], json!("PAID"));
    let exp = app
        .call(
            Method::POST,
            &format!("/api/bank-lines/{}/expense", id(charges)),
            Some(&admin),
            Some(json!({"expenseAccountId": opex})),
        )
        .await?;
    assert_eq!(exp.status, StatusCode::OK, "{}", exp.body);
    let tb = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    let cash_row = tb.body.as_array().and_then(|r| r.iter().find(|x| x["account"]["code"] == "1000")).cloned();
    assert_eq!(cash_row.map(|r| r["balance"].clone()), Some(json!(750)), "{}", tb.body);

    // A matched import cannot be undone; once unmatched it can (the payment and expense stay).
    let blocked = app.call(Method::DELETE, &format!("/api/imports/{import_id}"), Some(&admin), None).await?;
    assert_eq!(blocked.status, StatusCode::BAD_REQUEST, "{}", blocked.body);
    for l in [deposit, charges] {
        let u = app.call(Method::POST, &format!("/api/bank-lines/{}/unmatch", id(l)), Some(&admin), None).await?;
        assert_eq!(u.body["status"], json!("UNMATCHED"));
    }
    let undone = app.call(Method::DELETE, &format!("/api/imports/{import_id}"), Some(&admin), None).await?;
    assert_eq!(undone.body["status"], json!("UNDONE"), "{}", undone.body);
    let all = app.call(Method::GET, &format!("/api/bank-lines?accountId={cash}"), Some(&admin), None).await?;
    assert_eq!(all.body["data"], json!([]));
    let pays = app.call(Method::GET, "/api/payments", Some(&admin), None).await?;
    assert_eq!(pays.body["total"], json!(1), "unmatching never removes the payment");

    // Customers: new names only.
    let people = app
        .call(
            Method::POST,
            "/api/imports",
            Some(&admin),
            Some(json!({"kind": "CUSTOMERS", "source": "Old system", "rows": [
                {"name": "abc traders "}, {"name": "New Co", "email": "not-an-email"}, {"name": "New Co", "email": "a@newco.lk"}]})),
        )
        .await?;
    assert_eq!(
        (people.body["created"].clone(), people.body["skipped"].clone(), people.body["errors"].clone()),
        (json!(1), json!(1), json!(1)),
        "{}",
        people.body
    );

    // Invoices: rows with one number are one draft; an unknown customer is created; undo removes both.
    let sales = json!({"kind": "INVOICES", "source": "Shop", "defaultAccountId": revenue, "dateOrder": "DMY", "rows": [
        {"number": "S-1", "customer": "Walk-in Shop", "date": "10/10/2026", "description": "Tea", "quantity": "2", "unitPrice": "150"},
        {"number": "S-1", "customer": "Walk-in Shop", "date": "10/10/2026", "description": "Cake", "unitPrice": "200.50"},
        {"number": "S-2", "customer": "Walk-in Shop", "date": "someday", "description": "Tea", "unitPrice": "150"}
    ]});
    let s = app.call(Method::POST, "/api/imports", Some(&admin), Some(sales.clone())).await?;
    assert_eq!((s.body["created"].clone(), s.body["errors"].clone()), (json!(1), json!(1)), "{}", s.body);
    let drafts = app.call(Method::GET, "/api/invoices?status=DRAFT", Some(&admin), None).await?;
    let draft = drafts.body["data"].as_array().and_then(|d| d.first()).cloned().ok_or("draft")?;
    assert_eq!((draft["totalAmount"].as_str(), draft["status"].as_str()), (Some("500.50"), Some("DRAFT")));
    let s2 = app.call(Method::POST, "/api/imports", Some(&admin), Some(sales)).await?;
    assert_eq!(s2.body["skipped"], json!(1), "S-1 was imported before");
    let sid = s.body["id"].as_str().ok_or("sales import")?;
    let u = app.call(Method::DELETE, &format!("/api/imports/{sid}"), Some(&admin), None).await?;
    assert_eq!(u.status, StatusCode::OK, "{}", u.body);
    let custs = app.call(Method::GET, "/api/customers", Some(&admin), None).await?;
    assert!(custs.body.as_array().is_some_and(|c| c.iter().all(|x| x["name"] != "Walk-in Shop")));

    // Layouts are saved by name and replaced when saved again.
    let lay = json!({"kind": "BANK", "name": "Commercial Bank", "layout": {"date": "Date", "amount": "Amount"}});
    app.call(Method::PUT, "/api/imports/layouts", Some(&admin), Some(lay.clone())).await?;
    app.call(Method::PUT, "/api/imports/layouts", Some(&admin), Some(lay)).await?;
    let lays = app.call(Method::GET, "/api/imports/layouts?kind=BANK", Some(&admin), None).await?;
    assert_eq!(lays.body.as_array().map(Vec::len), Some(1));

    // A Viewer cannot import; the log has the import but not the preview.
    let viewer = app.signed_in("viewer", "Viewer").await?;
    let denied = app.call(Method::POST, "/api/imports/preview", Some(&viewer), Some(bank)).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let actions: Vec<&str> =
        log.body["data"].as_array().ok_or("log")?.iter().filter_map(|r| r["action"].as_str()).collect();
    assert!(actions.contains(&"Imported records from a file") && actions.contains(&"Undid an import"), "{actions:?}");
    assert!(actions.contains(&"Recorded a payment from a bank transaction"));
    assert!(!log.body.to_string().contains("/imports/preview"));
    Ok(())
}

#[tokio::test]
async fn products_stock_at_average_cost_and_invoice_discounts() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"region.currency": "LKR"}))).await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let code_id = |code: &str| -> String {
        accounts
            .body
            .as_array()
            .and_then(|a| a.iter().find(|x| x["code"] == code))
            .and_then(|x| x["id"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let (revenue, opex, equity) = (code_id("4000"), code_id("6000"), code_id("3000"));
    let id = |v: &Value| v["id"].as_str().unwrap_or_default().to_string();
    // Trial-balance figure of an account by code (in its normal direction: revenue positive).
    let tb = |body: &Value, code: &str| -> f64 {
        body.as_array()
            .and_then(|r| r.iter().find(|x| x["account"]["code"] == code))
            .and_then(|r| r["balance"].as_f64())
            .unwrap_or(0.0)
    };

    let tea = app
        .call(
            Method::POST,
            "/api/items",
            Some(&admin),
            Some(json!({"name": "Tea 400g", "sku": "T400", "type": "INVENTORY", "salePrice": 850, "reorderPoint": 5})),
        )
        .await?;
    assert_eq!(tea.status, StatusCode::CREATED, "{}", tea.body);
    assert!(tea.body["inventoryAccountId"].is_string() && tea.body["cogsAccountId"].is_string(), "{}", tea.body);
    let tea_id = id(&tea.body);
    let twin = app.call(Method::POST, "/api/items", Some(&admin), Some(json!({"name": " tea 400G "}))).await?;
    assert_eq!(twin.status, StatusCode::BAD_REQUEST, "names are unique");

    // Two purchases: 10 at 500 and 10 at 600 → 20 worth 11,000 (average 550).
    let vendor = app.call(Method::POST, "/api/vendors", Some(&admin), Some(json!({"name": "Estate Ltd"}))).await?;
    let mut bills = Vec::new();
    for price in [500, 600] {
        let b = app
            .call(
                Method::POST,
                "/api/bills",
                Some(&admin),
                Some(json!({"vendorId": id(&vendor.body), "date": "2026-10-01",
                    "lines": [{"description": "Tea", "quantity": 10, "unitPrice": price, "accountId": opex, "itemId": tea_id}]})),
            )
            .await?;
        assert_eq!(b.status, StatusCode::CREATED, "{}", b.body);
        let bid = id(&b.body);
        let p = app.call(Method::POST, &format!("/api/bills/{bid}/post"), Some(&admin), None).await?;
        assert_eq!(p.status, StatusCode::CREATED, "{}", p.body);
        bills.push(bid);
    }
    let item = app.call(Method::GET, &format!("/api/items/{tea_id}"), Some(&admin), None).await?;
    assert_eq!((item.body["quantityOnHand"].clone(), item.body["stockValue"].clone()), (json!(20), json!(11000)));
    assert_eq!(item.body["averageCost"], json!(550));
    let t = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!((tb(&t.body, "1200"), tb(&t.body, "6000")), (11000.0, 0.0), "stock lines go to inventory, not expense");

    // Sell 3 with a 10% discount and VAT 10% on top.
    let vat_acct = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(json!({"code": "2300", "name": "VAT", "type": "LIABILITY"})),
        )
        .await?;
    let vat = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "VAT 10%", "rate": 10, "salesAccountId": id(&vat_acct.body), "purchaseAccountId": id(&vat_acct.body)})),
        )
        .await?;
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "Corner Shop"}))).await?;
    let sale = |qty: i64| {
        json!({"customerId": id(&cust.body), "date": "2026-10-05", "lines": [{"description": "Tea 400g", "quantity": qty,
            "unitPrice": 850, "discountAmount": 255, "itemId": tea_id, "taxCodeId": id(&vat.body)}]})
    };
    let inv = app.call(Method::POST, "/api/invoices", Some(&admin), Some(sale(3))).await?;
    assert_eq!(inv.status, StatusCode::CREATED, "{}", inv.body);
    assert_eq!(inv.body["lines"][0]["accountId"], json!(revenue), "the item's income account is used");
    let inv_id = id(&inv.body);
    let read = app.call(Method::GET, &format!("/api/invoices/{inv_id}"), Some(&admin), None).await?;
    // 3 × 850 = 2,550 − 255 = 2,295 + VAT 229.50 = 2,524.50.
    assert_eq!((read.body["totalAmount"].as_str(), read.body["taxAmount"].as_str()), (Some("2524.50"), Some("229.50")));
    assert_eq!(read.body["lines"][0]["discountAmount"], json!("255.00"));
    let too_big = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": id(&cust.body), "date": "2026-10-05",
        "lines": [{"description": "x", "quantity": 1, "unitPrice": 10, "discountAmount": 11, "accountId": revenue}]})),
        )
        .await?;
    assert_eq!(too_big.status, StatusCode::BAD_REQUEST, "a discount cannot exceed the line");
    let posted = app.call(Method::POST, &format!("/api/invoices/{inv_id}/post"), Some(&admin), None).await?;
    assert_eq!(posted.status, StatusCode::CREATED, "{}", posted.body);
    let t = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t.body, "5000"), 1650.0, "COGS = 3 × 550: {}", t.body);
    assert_eq!(tb(&t.body, "1200"), 9350.0);
    assert_eq!(tb(&t.body, "4000"), 2550.0, "revenue is the price before the discount");
    assert_eq!(tb(&t.body, "4900"), -255.0, "the discount is its own (contra-revenue) account");
    assert_eq!(tb(&t.body, "1100"), 2524.5);

    // Prices including tax: 1,100 incl. VAT less 110 → 990 charged (VAT 90). The discount without
    // its tax (100) goes to 4900 and revenue is 1,000 (the price without tax).
    let incl = app
        .call(
            Method::POST,
            "/api/invoices",
            Some(&admin),
            Some(json!({"customerId": id(&cust.body), "date": "2026-10-05", "pricesIncludeTax": true,
                "lines": [{"description": "Service", "quantity": 1, "unitPrice": 1100, "discountAmount": 110,
                           "accountId": revenue, "taxCodeId": id(&vat.body)}]})),
        )
        .await?;
    let p = app.call(Method::POST, &format!("/api/invoices/{}/post", id(&incl.body)), Some(&admin), None).await?;
    assert_eq!(p.status, StatusCode::CREATED, "{}", p.body);
    let t = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!((tb(&t.body, "4000"), tb(&t.body, "4900"), tb(&t.body, "2300")), (3550.0, -355.0, 319.5));

    // Selling more than is in stock is refused; the draft stays.
    let big = app.call(Method::POST, "/api/invoices", Some(&admin), Some(sale(50))).await?;
    let refused = app.call(Method::POST, &format!("/api/invoices/{}/post", id(&big.body)), Some(&admin), None).await?;
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{}", refused.body);
    assert!(refused.body["message"]
        .as_str()
        .is_some_and(|m| m.starts_with("Not enough stock of Tea 400g: 17 on hand, 50 needed")));

    // Voiding the sale puts the stock back at what it cost.
    let v = app.call(Method::POST, &format!("/api/invoices/{inv_id}/void"), Some(&admin), None).await?;
    assert_eq!(v.status, StatusCode::CREATED, "{}", v.body);
    let item = app.call(Method::GET, &format!("/api/items/{tea_id}"), Some(&admin), None).await?;
    assert_eq!((item.body["quantityOnHand"].clone(), item.body["stockValue"].clone()), (json!(20), json!(11000)));

    // Adjustments: 2 lost (at average cost, to 6000), 5 found at 600 (against equity).
    let lost = app
        .call(
            Method::POST,
            &format!("/api/items/{tea_id}/adjust"),
            Some(&admin),
            Some(json!({"date": "2026-10-06", "quantity": -2, "accountId": opex, "note": "Stock count"})),
        )
        .await?;
    assert_eq!(
        (lost.body["quantityOnHand"].clone(), lost.body["stockValue"].clone()),
        (json!(18), json!(9900)),
        "{}",
        lost.body
    );
    let found = app
        .call(
            Method::POST,
            &format!("/api/items/{tea_id}/adjust"),
            Some(&admin),
            Some(json!({"date": "2026-10-06", "quantity": 5, "unitCost": 600, "accountId": equity})),
        )
        .await?;
    assert_eq!(found.body["stockValue"], json!(12900));
    // Voiding the first bill takes out exactly what it brought in.
    let vb = app.call(Method::POST, &format!("/api/bills/{}/void", bills[0]), Some(&admin), None).await?;
    assert_eq!(vb.status, StatusCode::CREATED, "{}", vb.body);
    let item = app.call(Method::GET, &format!("/api/items/{tea_id}"), Some(&admin), None).await?;
    assert_eq!((item.body["quantityOnHand"].clone(), item.body["stockValue"].clone()), (json!(13), json!(7900)));
    let t = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    assert_eq!(tb(&t.body, "1200"), 7900.0, "the inventory account always equals the stock value");
    let moves = app.call(Method::GET, &format!("/api/items/{tea_id}/moves"), Some(&admin), None).await?;
    assert_eq!(moves.body.as_array().map(Vec::len), Some(7), "{}", moves.body);

    // A used item cannot be deleted, only made inactive; a Viewer cannot add items.
    let del = app.call(Method::DELETE, &format!("/api/items/{tea_id}"), Some(&admin), None).await?;
    assert_eq!(del.status, StatusCode::BAD_REQUEST);
    let off = app
        .call(Method::PATCH, &format!("/api/items/{tea_id}"), Some(&admin), Some(json!({"isActive": false})))
        .await?;
    assert_eq!(off.body["isActive"], json!(false));
    let retype = app
        .call(Method::PATCH, &format!("/api/items/{tea_id}"), Some(&admin), Some(json!({"type": "SERVICE"})))
        .await?;
    assert_eq!(retype.status, StatusCode::BAD_REQUEST, "a stock item that has moved stays one");
    let viewer = app.signed_in("viewer", "Viewer").await?;
    let denied = app.call(Method::POST, "/api/items", Some(&viewer), Some(json!({"name": "X"}))).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    Ok(())
}

/// A stand-in for the Shopify Admin API: a shop, two pages of orders and Shopify Payments.
async fn fake_shopify() -> Result<String, Box<dyn std::error::Error>> {
    use axum::extract::{Query, Request};
    use axum::response::IntoResponse;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let authorised = |req: &Request| {
        req.headers().get("X-Shopify-Access-Token").and_then(|v| v.to_str().ok()) == Some("shpat_test_1234567890")
    };
    let page_base = base.clone();
    let app = Router::new()
        .route(
            "/admin/api/{version}/shop.json",
            axum::routing::get(move |req: Request| async move {
                if !authorised(&req) {
                    return (StatusCode::UNAUTHORIZED, "no").into_response();
                }
                axum::Json(json!({"shop": {"name": "Hill Tea Store", "currency": "LKR"}})).into_response()
            }),
        )
        .route(
            "/admin/api/{version}/orders.json",
            axum::routing::get(move |Query(q): Query<std::collections::HashMap<String, String>>, req: Request| {
                let page_base = page_base.clone();
                async move {
                    if !authorised(&req) {
                        return (StatusCode::UNAUTHORIZED, "no").into_response();
                    }
                    if q.contains_key("page_info") {
                        // Page 2: an order with a tax the app does not know yet.
                        return axum::Json(json!({"orders": [{
                            "id": 5003, "name": "#1003", "processed_at": "2026-10-11T09:00:00+05:30", "currency": "LKR",
                            "financial_status": "paid", "cancelled_at": null, "taxes_included": false, "total_price": "1025.00",
                            "customer": {"first_name": "Kamal", "last_name": "Silva", "email": "kamal@example.lk"},
                            "line_items": [{"title": "Green Tea", "sku": "", "quantity": 1, "price": "1000.00",
                                "discount_allocations": [], "tax_lines": [{"title": "SSCL", "rate": 0.025, "price": "25.00"}]}],
                            "shipping_lines": []
                        }]}))
                        .into_response();
                    }
                    let next = format!("<{page_base}/admin/api/2026-07/orders.json?page_info=p2&limit=250>; rel=\"next\"");
                    (
                        [("Link", next)],
                        axum::Json(json!({"orders": [
                            {"id": 5001, "name": "#1001", "processed_at": "2026-10-10T10:00:00+05:30", "currency": "LKR",
                             "financial_status": "paid", "cancelled_at": null, "taxes_included": false, "total_price": "2155.40",
                             "customer": {"first_name": "Jane", "last_name": "Perera", "email": "jane@example.lk"},
                             "line_items": [{"title": "Ceylon Tea", "variant_title": "400g", "sku": "T400", "quantity": 2, "price": "850.00",
                                 "discount_allocations": [{"amount": "170.00"}],
                                 "tax_lines": [{"title": "VAT", "rate": 0.18, "price": "275.40"}]}],
                             "shipping_lines": [{"title": "Standard", "price": "350.00", "discounted_price": "350.00", "tax_lines": []}]},
                            {"id": 5002, "name": "#1002", "processed_at": "2026-10-10T11:00:00+05:30", "currency": "LKR",
                             "financial_status": "voided", "cancelled_at": "2026-10-10T12:00:00+05:30", "total_price": "100.00",
                             "line_items": [{"title": "X", "quantity": 1, "price": "100.00"}], "shipping_lines": []}
                        ]})),
                    )
                        .into_response()
                }
            }),
        )
        .route(
            "/admin/api/{version}/shopify_payments/balance/transactions.json",
            axum::routing::get(move |req: Request| async move {
                if !authorised(&req) {
                    return (StatusCode::UNAUTHORIZED, "no").into_response();
                }
                axum::Json(json!({"transactions": [
                    {"id": 9001, "type": "charge", "test": false, "amount": "2155.40", "fee": "64.66", "source_order_id": 5001,
                     "processed_at": "2026-10-10T10:05:00+05:30", "currency": "LKR"},
                    {"id": 9002, "type": "payout", "test": false, "amount": "-2090.74", "fee": "0.00", "source_order_id": null,
                     "processed_at": "2026-10-12T08:00:00+05:30", "currency": "LKR"},
                    {"id": 9003, "type": "charge", "test": true, "amount": "1.00", "fee": "0.00", "processed_at": "2026-10-12T08:00:00+05:30"}
                ]}))
                .into_response()
            }),
        );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(base)
}

#[tokio::test]
async fn shopify_orders_become_drafts_and_payments_bank_lines() -> TestResult {
    let base = fake_shopify().await?;
    std::env::set_var("ACCOUNTING_SHOPIFY_BASE_URL", &base);
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"region.currency": "LKR"}))).await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let code_id = |code: &str| -> String {
        accounts
            .body
            .as_array()
            .and_then(|a| a.iter().find(|x| x["code"] == code))
            .and_then(|x| x["id"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let (cash, revenue, opex) = (code_id("1000"), code_id("4000"), code_id("6000"));
    let id = |v: &Value| v["id"].as_str().unwrap_or_default().to_string();

    // A wrong key is refused; the right one connects and is never shown again.
    let bad = app
        .call(
            Method::POST,
            "/api/integrations/shopify/connect",
            Some(&admin),
            Some(json!({"shop": "hilltea", "token": "shpat_wrong_000000"})),
        )
        .await?;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.body);
    let ok = app
        .call(
            Method::POST,
            "/api/integrations/shopify/connect",
            Some(&admin),
            Some(json!({"shop": "https://HillTea.myshopify.com/admin", "token": "shpat_test_1234567890"})),
        )
        .await?;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert_eq!(
        (ok.body["connected"].clone(), ok.body["remoteId"].clone()),
        (json!(true), json!("hilltea.myshopify.com"))
    );
    assert_eq!(ok.body["remoteName"], json!("Hill Tea Store"));
    assert!(!ok.body.to_string().contains("shpat_"), "the key never comes back");

    // The matching: a clearing account, payouts to Cash, fees to 6000, VAT 18% to a tax code.
    let clearing = app.call(Method::POST, "/api/integrations/shopify/clearing-account", Some(&admin), None).await?;
    let clearing_id = clearing.body["clearingAccountId"].as_str().ok_or("clearing")?.to_string();
    let vat_acct = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(json!({"code": "2300", "name": "VAT", "type": "LIABILITY"})),
        )
        .await?;
    let vat = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "VAT 18%", "rate": 18, "salesAccountId": id(&vat_acct.body), "purchaseAccountId": id(&vat_acct.body)})),
        )
        .await?;
    let item = app
        .call(
            Method::POST,
            "/api/items",
            Some(&admin),
            Some(json!({"name": "Ceylon Tea 400g", "sku": "t400", "type": "SERVICE"})),
        )
        .await?;
    let set = app
        .call(
            Method::PUT,
            "/api/integrations/shopify",
            Some(&admin),
            Some(json!({"startDate": "2026-10-01", "payoutAccountId": cash, "feeAccountId": opex, "incomeAccountId": revenue,
                "taxMap": {"VAT|0.18": id(&vat.body)}})),
        )
        .await?;
    assert_eq!(set.status, StatusCode::OK, "{}", set.body);
    let wrong = app
        .call(Method::PUT, "/api/integrations/shopify", Some(&admin), Some(json!({"feeAccountId": revenue})))
        .await?;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST, "fees go to an expense account");

    // First sync: #1001 comes in (two pages read), #1002 is cancelled, #1003's tax is not matched yet.
    let sync = app.call(Method::POST, "/api/integrations/shopify/sync", Some(&admin), None).await?;
    assert_eq!(sync.status, StatusCode::OK, "{}", sync.body);
    assert_eq!(sync.body["orders"], json!({"created": 1, "skipped": 1, "errors": 1}), "{}", sync.body);
    assert_eq!(sync.body["unmappedTaxes"], json!(["SSCL|0.025"]));
    assert_eq!(sync.body["lines"]["created"], json!(3), "payment, fee and payout; the test charge is left out");
    let invs = app.call(Method::GET, "/api/invoices?status=DRAFT", Some(&admin), None).await?;
    let inv = invs.body["data"].as_array().and_then(|d| d.first()).cloned().ok_or("draft")?;
    assert_eq!((inv["poNumber"].as_str(), inv["totalAmount"].as_str()), (Some("#1001"), Some("2155.40")), "{inv}");
    let full = app.call(Method::GET, &format!("/api/invoices/{}", id(&inv)), Some(&admin), None).await?;
    let lines = full.body["lines"].as_array().cloned().unwrap_or_default();
    assert_eq!(lines[0]["description"], json!("Ceylon Tea — 400g"));
    assert_eq!(lines[0]["itemId"], json!(id(&item.body)), "matched by SKU, whatever the case");
    assert_eq!(lines[0]["discountAmount"], json!("170.00"));
    assert_eq!(lines[1]["description"], json!("Shipping: Standard"));
    assert_eq!(full.body["customer"]["name"], json!("Jane Perera"));

    // Second sync, with SSCL matched: only #1003 is new; nothing comes in twice.
    let sscl = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "SSCL 2.5%", "rate": 2.5, "salesAccountId": id(&vat_acct.body), "purchaseAccountId": id(&vat_acct.body)})),
        )
        .await?;
    app.call(
        Method::PUT,
        "/api/integrations/shopify",
        Some(&admin),
        Some(json!({"taxMap": {"VAT|0.18": id(&vat.body), "SSCL|0.025": id(&sscl.body)}})),
    )
    .await?;
    let again = app.call(Method::POST, "/api/integrations/shopify/sync", Some(&admin), None).await?;
    assert_eq!(again.body["orders"], json!({"created": 1, "skipped": 2, "errors": 0}), "{}", again.body);
    assert_eq!(again.body["lines"], json!({"created": 0, "skipped": 3}));

    // Post #1001, then work the clearing account: the payment pays it, the fee is an expense,
    // the payout is a transfer to Cash.
    let post = app.call(Method::POST, &format!("/api/invoices/{}/post", id(&inv)), Some(&admin), None).await?;
    assert_eq!(post.status, StatusCode::CREATED, "{}", post.body);
    let bl = app
        .call(Method::GET, &format!("/api/bank-lines?accountId={clearing_id}&status=UNMATCHED"), Some(&admin), None)
        .await?;
    let data = bl.body["data"].as_array().cloned().unwrap_or_default();
    let find = |d: &str| data.iter().find(|l| l["description"].as_str().is_some_and(|x| x.starts_with(d))).cloned();
    let payment = find("Shopify payment #1001").ok_or("payment line")?;
    let fee = find("Shopify fee #1001").ok_or("fee line")?;
    let payout = find("Shopify payout").ok_or("payout line")?;
    assert_eq!(payment["suggestions"][0]["id"], json!(id(&inv)), "{payment}");
    assert_eq!(fee["hint"], json!({"action": "EXPENSE", "accountId": opex}));
    assert_eq!(payout["hint"], json!({"action": "TRANSFER", "accountId": cash}));
    for (line, path, body) in [
        (&payment, "payment", json!({"invoiceId": id(&inv)})),
        (&fee, "expense", json!({"expenseAccountId": opex})),
        (&payout, "transfer", json!({"accountId": cash})),
    ] {
        let r =
            app.call(Method::POST, &format!("/api/bank-lines/{}/{path}", id(line)), Some(&admin), Some(body)).await?;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.body);
    }
    let tb = app.call(Method::GET, "/api/ledger/trial-balance", Some(&admin), None).await?;
    let bal = |code: &str| {
        tb.body
            .as_array()
            .and_then(|r| r.iter().find(|x| x["account"]["code"] == code))
            .and_then(|r| r["balance"].as_f64())
            .unwrap_or(0.0)
    };
    assert_eq!((bal("1090"), bal("1000"), bal("6000")), (0.0, 2090.74, 64.66), "{}", tb.body);

    // Cash's own statement later shows the payout: it is suggested against the same transfer.
    let stmt = app
        .call(
            Method::POST,
            "/api/imports",
            Some(&admin),
            Some(json!({"kind": "BANK", "source": "Bank", "accountId": cash, "dateOrder": "YMD",
                "rows": [{"date": "2026-10-12", "description": "SHOPIFY PAYOUT", "amount": "2090.74"}]})),
        )
        .await?;
    assert_eq!(stmt.body["created"], json!(1));
    let cl = app
        .call(Method::GET, &format!("/api/bank-lines?accountId={cash}&status=UNMATCHED"), Some(&admin), None)
        .await?;
    let line = cl.body["data"][0].clone();
    assert_eq!(line["suggestions"][0]["type"], json!("TRANSFER"), "{line}");
    let m = app
        .call(
            Method::POST,
            &format!("/api/bank-lines/{}/match", id(&line)),
            Some(&admin),
            Some(json!({"type": "TRANSFER", "id": line["suggestions"][0]["id"]})),
        )
        .await?;
    assert_eq!(m.body["status"], json!("MATCHED"), "{}", m.body);

    // Disconnecting forgets the key; a sync then asks to connect first.
    let off = app.call(Method::DELETE, "/api/integrations/shopify", Some(&admin), None).await?;
    assert_eq!(off.body["connected"], json!(false));
    let no = app.call(Method::POST, "/api/integrations/shopify/sync", Some(&admin), None).await?;
    assert_eq!(no.status, StatusCode::BAD_REQUEST);
    let viewer = app.signed_in("viewer", "Viewer").await?;
    let denied = app.call(Method::GET, "/api/integrations/shopify", Some(&viewer), None).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let text = log.body.to_string();
    assert!(text.contains("Connected Shopify") && text.contains("Synced with Shopify") && !text.contains("shpat_"));
    Ok(())
}

/// Stand-ins for WooCommerce, Stripe and PayPal on one local server.
async fn fake_services() -> Result<String, Box<dyn std::error::Error>> {
    use axum::extract::{Query, Request};
    use axum::response::IntoResponse;
    type Q = Query<std::collections::HashMap<String, String>>;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let header =
        |req: &Request, name: &str| req.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    // ck_test_key:cs_test_secret, as HTTP Basic.
    let woo_ok = move |req: &Request, q: &std::collections::HashMap<String, String>| {
        header(req, "authorization") == "Basic Y2tfdGVzdF9rZXk6Y3NfdGVzdF9zZWNyZXQ="
            || q.get("consumer_key").map(String::as_str) == Some("ck_test_key")
    };
    let stripe_ok = move |req: &Request| header(req, "authorization") == "Bearer sk_test_abcdefghijklmnop1234";
    let day = |d: u32| {
        chrono::NaiveDate::from_ymd_opt(2026, 10, d)
            .and_then(|x| x.and_hms_opt(6, 0, 0))
            .map(|x| x.and_utc().timestamp())
    };
    let (d10, d12) = (day(10).unwrap_or_default(), day(12).unwrap_or_default());
    let app = Router::new()
        .route(
            "/wp-json/wc/v3/settings/general",
            axum::routing::get(move |Query(q): Q, req: Request| async move {
                if !woo_ok(&req, &q) {
                    return (StatusCode::UNAUTHORIZED, "{}").into_response();
                }
                axum::Json(json!([{"id": "woocommerce_currency", "value": "LKR"}])).into_response()
            }),
        )
        .route(
            "/wp-json/wc/v3/orders",
            axum::routing::get(move |Query(q): Q, req: Request| async move {
                if !woo_ok(&req, &q) {
                    return (StatusCode::UNAUTHORIZED, "{}").into_response();
                }
                (
                    [("X-WP-TotalPages", "1")],
                    axum::Json(json!([
                        {"id": 77, "number": "1077", "status": "completed", "currency": "LKR", "date_created": "2026-10-10T10:00:00",
                         "total": "2360.00", "billing": {"first_name": "Ruwan", "last_name": "Jay", "email": "ruwan@example.lk"},
                         "tax_lines": [{"rate_id": 5, "label": "VAT", "rate_percent": 18}],
                         "line_items": [{"name": "Tea", "sku": "", "quantity": 2, "subtotal": "2000.00", "total": "1800.00",
                                         "taxes": [{"id": 5, "total": "324.00"}]}],
                         "shipping_lines": [{"method_title": "Courier", "total": "200.00", "taxes": [{"id": 5, "total": "36.00"}]}]},
                        {"id": 78, "number": "1078", "status": "pending", "currency": "LKR", "date_created": "2026-10-11T10:00:00",
                         "total": "100.00", "billing": {}, "line_items": []}
                    ])),
                )
                    .into_response()
            }),
        )
        .route(
            "/v1/balance",
            axum::routing::get(move |req: Request| async move {
                if !stripe_ok(&req) {
                    return (StatusCode::UNAUTHORIZED, "{}").into_response();
                }
                axum::Json(json!({"available": [{"currency": "lkr", "amount": 0}]})).into_response()
            }),
        )
        .route("/v1/account", axum::routing::get(|| async { axum::Json(json!({"id": "acct_1", "settings": {"dashboard": {"display_name": "Tea Ltd"}}})) }))
        .route(
            "/v1/balance_transactions",
            axum::routing::get(move |Query(q): Q, req: Request| async move {
                if !stripe_ok(&req) {
                    return (StatusCode::UNAUTHORIZED, "{}").into_response();
                }
                if q.get("starting_after").map(String::as_str) == Some("txn_1") {
                    return axum::Json(json!({"has_more": false, "data": [
                        {"id": "txn_2", "type": "payout", "amount": -228920, "fee": 0, "currency": "lkr", "created": d12, "source": "po_1"}]}))
                    .into_response();
                }
                axum::Json(json!({"has_more": true, "data": [
                    {"id": "txn_1", "type": "charge", "amount": 236000, "fee": 7080, "currency": "lkr", "created": d10,
                     "description": "Order 1077", "source": {"id": "ch_1", "billing_details": {"name": "Ruwan Jay"}}}]}))
                .into_response()
            }),
        )
        .route(
            "/v1/oauth2/token",
            axum::routing::post(move |req: Request| async move {
                // client id "AppClientId123" : secret "AppSecret12345".
                if header(&req, "authorization") != "Basic QXBwQ2xpZW50SWQxMjM6QXBwU2VjcmV0MTIzNDU=" {
                    return (StatusCode::UNAUTHORIZED, "{}").into_response();
                }
                axum::Json(json!({"access_token": "A21AA", "token_type": "Bearer"})).into_response()
            }),
        )
        .route("/v1/reporting/balances", axum::routing::get(|| async { axum::Json(json!({"balances": [{"currency": "USD", "primary": true}]})) }))
        .route(
            "/v1/reporting/transactions",
            axum::routing::get(move |req: Request| async move {
                if header(&req, "authorization") != "Bearer A21AA" {
                    return (StatusCode::UNAUTHORIZED, "{}").into_response();
                }
                axum::Json(json!({"total_pages": 1, "transaction_details": [
                    {"transaction_info": {"transaction_id": "9AB", "transaction_event_code": "T0006", "transaction_status": "S",
                        "transaction_initiation_date": "2026-10-10T05:00:00+0000", "invoice_id": "INV-77",
                        "transaction_amount": {"currency_code": "USD", "value": "100.00"}, "fee_amount": {"currency_code": "USD", "value": "-4.20"}},
                     "payer_info": {"payer_name": {"given_name": "Ann", "surname": "Lee"}}},
                    {"transaction_info": {"transaction_id": "W1", "transaction_event_code": "T0400", "transaction_status": "S",
                        "transaction_initiation_date": "2026-10-11T05:00:00+0000",
                        "transaction_amount": {"currency_code": "USD", "value": "-95.80"}}}
                ]}))
                .into_response()
            }),
        );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok(base)
}

#[tokio::test]
async fn woocommerce_orders_paid_through_stripe_and_paypal_activity() -> TestResult {
    let base = fake_services().await?;
    for id in ["WOOCOMMERCE", "STRIPE", "PAYPAL"] {
        std::env::set_var(format!("ACCOUNTING_{id}_BASE_URL"), &base);
    }
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::PATCH, "/api/settings", Some(&admin), Some(json!({"region.currency": "LKR"}))).await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let accounts = app.call(Method::GET, "/api/accounts", Some(&admin), None).await?;
    let code_id = |code: &str| -> String {
        accounts
            .body
            .as_array()
            .and_then(|a| a.iter().find(|x| x["code"] == code))
            .and_then(|x| x["id"].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let (cash, revenue, opex) = (code_id("1000"), code_id("4000"), code_id("6000"));
    let id = |v: &Value| v["id"].as_str().unwrap_or_default().to_string();
    let vat_acct = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(json!({"code": "2300", "name": "VAT", "type": "LIABILITY"})),
        )
        .await?;
    let vat = app
        .call(
            Method::POST,
            "/api/tax-codes",
            Some(&admin),
            Some(json!({"name": "VAT 18%", "rate": 18, "salesAccountId": id(&vat_acct.body), "purchaseAccountId": id(&vat_acct.body)})),
        )
        .await?;

    // All four integrations are listed.
    let list = app.call(Method::GET, "/api/integrations", Some(&admin), None).await?;
    let ids: Vec<&str> = list.body.as_array().ok_or("list")?.iter().filter_map(|x| x["id"].as_str()).collect();
    assert_eq!(ids, ["shopify", "woocommerce", "paypal", "stripe"]);

    // WooCommerce: plain http is refused; the key is checked; a completed order becomes a draft.
    let insecure = app
        .call(
            Method::POST,
            "/api/integrations/woocommerce/connect",
            Some(&admin),
            Some(json!({"site": "http://shop.example.lk", "consumerKey": "ck_test_key", "consumerSecret": "cs_test_secret"})),
        )
        .await?;
    assert_eq!(insecure.status, StatusCode::BAD_REQUEST);
    let woo = app
        .call(
            Method::POST,
            "/api/integrations/woocommerce/connect",
            Some(&admin),
            Some(json!({"site": "shop.example.lk", "consumerKey": "ck_test_key", "consumerSecret": "cs_test_secret"})),
        )
        .await?;
    assert_eq!(woo.status, StatusCode::OK, "{}", woo.body);
    assert_eq!(
        (woo.body["remoteId"].as_str(), woo.body["remoteCurrency"].as_str()),
        (Some("https://shop.example.lk"), Some("LKR"))
    );
    app.call(
        Method::PUT,
        "/api/integrations/woocommerce",
        Some(&admin),
        Some(json!({"startDate": "2026-10-01", "incomeAccountId": revenue, "taxMap": {"VAT|0.18": id(&vat.body)}})),
    )
    .await?;
    let ws = app.call(Method::POST, "/api/integrations/woocommerce/sync", Some(&admin), None).await?;
    assert_eq!(ws.body["orders"], json!({"created": 1, "skipped": 1, "errors": 0}), "{}", ws.body);
    let invs = app.call(Method::GET, "/api/invoices?status=DRAFT", Some(&admin), None).await?;
    let inv = invs.body["data"].as_array().and_then(|d| d.first()).cloned().ok_or("draft")?;
    assert_eq!((inv["poNumber"].as_str(), inv["totalAmount"].as_str()), (Some("#1077"), Some("2360.00")), "{inv}");
    let post = app.call(Method::POST, &format!("/api/invoices/{}/post", id(&inv)), Some(&admin), None).await?;
    assert_eq!(post.status, StatusCode::CREATED, "{}", post.body);

    // Stripe: a publishable key is refused; the charge (two pages), its fee and the payout come in.
    let pk = app
        .call(
            Method::POST,
            "/api/integrations/stripe/connect",
            Some(&admin),
            Some(json!({"secretKey": "pk_test_abcdefghijklmnop1234"})),
        )
        .await?;
    assert_eq!(pk.status, StatusCode::BAD_REQUEST);
    let st = app
        .call(
            Method::POST,
            "/api/integrations/stripe/connect",
            Some(&admin),
            Some(json!({"secretKey": "sk_test_abcdefghijklmnop1234"})),
        )
        .await?;
    assert_eq!(st.status, StatusCode::OK, "{}", st.body);
    assert_eq!(st.body["remoteName"], json!("Tea Ltd (test mode)"));
    let money = app.call(Method::POST, "/api/integrations/stripe/clearing-account", Some(&admin), None).await?;
    let stripe_acct = money.body["clearingAccountId"].as_str().ok_or("stripe account")?.to_string();
    app.call(
        Method::PUT,
        "/api/integrations/stripe",
        Some(&admin),
        Some(json!({"startDate": "2026-10-01", "feeAccountId": opex, "payoutAccountId": cash})),
    )
    .await?;
    let ss = app.call(Method::POST, "/api/integrations/stripe/sync", Some(&admin), None).await?;
    assert_eq!(ss.body["lines"], json!({"created": 3, "skipped": 0}), "{}", ss.body);
    let bl = app
        .call(Method::GET, &format!("/api/bank-lines?accountId={stripe_acct}&status=UNMATCHED"), Some(&admin), None)
        .await?;
    let data = bl.body["data"].as_array().cloned().unwrap_or_default();
    let payment = data.iter().find(|l| l["amount"] == json!(2360)).cloned().ok_or("payment")?;
    assert_eq!(
        payment["suggestions"][0]["id"],
        json!(id(&inv)),
        "the Stripe payment pays the WooCommerce invoice: {payment}"
    );
    assert!(data.iter().any(|l| l["amount"] == json!(-70.8) && l["hint"]["action"] == "EXPENSE"));
    assert!(data.iter().any(|l| l["amount"] == json!(-2289.2) && l["hint"]["action"] == "TRANSFER"));
    let again = app.call(Method::POST, "/api/integrations/stripe/sync", Some(&admin), None).await?;
    assert_eq!(again.body["lines"], json!({"created": 0, "skipped": 3}));

    // PayPal: the balance is in USD, so the LKR money account takes nothing; a USD account does.
    let pp = app
        .call(
            Method::POST,
            "/api/integrations/paypal/connect",
            Some(&admin),
            Some(json!({"environment": "sandbox", "clientId": "AppClientId123", "clientSecret": "AppSecret12345"})),
        )
        .await?;
    assert_eq!(pp.status, StatusCode::OK, "{}", pp.body);
    assert_eq!(
        (pp.body["remoteCurrency"].as_str(), pp.body["options"]["environment"].as_str()),
        (Some("USD"), Some("sandbox"))
    );
    app.call(Method::POST, "/api/integrations/paypal/clearing-account", Some(&admin), None).await?;
    app.call(
        Method::PUT,
        "/api/integrations/paypal",
        Some(&admin),
        Some(json!({"startDate": "2026-10-01", "feeAccountId": opex})),
    )
    .await?;
    let lkr = app.call(Method::POST, "/api/integrations/paypal/sync", Some(&admin), None).await?;
    assert_eq!(lkr.body["lines"]["created"], json!(0));
    assert!(lkr.body["notes"].to_string().contains("another currency"), "{}", lkr.body);
    let usd = app
        .call(
            Method::POST,
            "/api/accounts",
            Some(&admin),
            Some(json!({"code": "1093", "name": "PayPal USD", "type": "ASSET", "currency": "USD"})),
        )
        .await?;
    app.call(Method::PUT, "/api/integrations/paypal", Some(&admin), Some(json!({"clearingAccountId": id(&usd.body)})))
        .await?;
    let ps = app.call(Method::POST, "/api/integrations/paypal/sync", Some(&admin), None).await?;
    assert_eq!(ps.body["lines"]["created"], json!(3), "payment, fee, withdrawal: {}", ps.body);
    let log = app.call(Method::GET, "/api/audit-logs?limit=100", Some(&admin), None).await?;
    let text = log.body.to_string();
    assert!(
        text.contains("Connected WooCommerce")
            && text.contains("Synced with Stripe")
            && text.contains("Connected PayPal")
    );
    assert!(!text.contains("cs_test_secret") && !text.contains("sk_test_abc") && !text.contains("AppSecret"));
    Ok(())
}

#[tokio::test]
async fn claude_keys_read_create_drafts_only_and_can_be_revoked() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("admin", "Admin").await?;
    app.call(Method::POST, "/api/accounts/seed", Some(&admin), None).await?;
    let cust = app.call(Method::POST, "/api/customers", Some(&admin), Some(json!({"name": "ABC Traders"}))).await?;
    let mk = |name: &str, write: bool| json!({"name": name, "canWrite": write});
    let ro = app.call(Method::POST, "/api/api-keys", Some(&admin), Some(mk("Claude reader", false))).await?;
    assert_eq!(ro.status, StatusCode::CREATED, "{}", ro.body);
    let ro_key = ro.body["key"].as_str().ok_or("key")?.to_string();
    assert!(ro_key.starts_with("cak_") && ro_key.len() == 44);
    let rw = app.call(Method::POST, "/api/api-keys", Some(&admin), Some(mk("Claude", true))).await?;
    let rw_key = rw.body["key"].as_str().ok_or("key")?.to_string();
    let listed = app.call(Method::GET, "/api/api-keys", Some(&admin), None).await?;
    assert!(!listed.body.to_string().contains(&ro_key), "a key is shown only once");

    // Read-only: reads, never writes; people, users and keys are closed.
    let me = app.call(Method::GET, "/api/api-keys/me", Some(ro_key.as_str()), None).await?;
    assert_eq!((me.body["canWrite"].clone(), me.body["user"].clone()), (json!(false), json!("admin")));
    assert_eq!(app.call(Method::GET, "/api/invoices", Some(ro_key.as_str()), None).await?.status, StatusCode::OK);
    let draft = json!({"customerId": cust.body["id"], "date": "2026-10-22", "lines": [{"description": "Work", "quantity": 1, "unitPrice": 100}]});
    let no = app.call(Method::POST, "/api/invoices", Some(ro_key.as_str()), Some(draft.clone())).await?;
    assert_eq!(no.status, StatusCode::FORBIDDEN);
    for closed in ["/api/employees", "/api/payroll/runs", "/api/users", "/api/api-keys", "/api/audit-logs"] {
        let r = app.call(Method::GET, closed, Some(ro_key.as_str()), None).await?;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{closed}");
    }

    // Write key: a draft yes; posting, paying and settings no.
    let inv = app.call(Method::POST, "/api/invoices", Some(rw_key.as_str()), Some(draft)).await?;
    assert_eq!(inv.status, StatusCode::CREATED, "{}", inv.body);
    assert_eq!(inv.body["status"], json!("DRAFT"));
    let inv_id = inv.body["id"].as_str().ok_or("id")?;
    for (method, path) in [
        (Method::POST, format!("/api/invoices/{inv_id}/post")),
        (Method::DELETE, format!("/api/invoices/{inv_id}")),
        (Method::PATCH, "/api/settings".to_string()),
        (Method::POST, "/api/payments".to_string()),
    ] {
        let r = app.call(method.clone(), &path, Some(rw_key.as_str()), Some(json!({}))).await?;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{method} {path}");
    }
    let log = app.call(Method::GET, "/api/audit-logs?limit=20", Some(&admin), None).await?;
    assert!(log.body.to_string().contains("admin via Claude"), "{}", log.body);

    // Revoked: refused at once.
    app.call(Method::DELETE, &format!("/api/api-keys/{}", id_of(&rw.body)), Some(&admin), None).await?;
    let gone = app.call(Method::GET, "/api/invoices", Some(rw_key.as_str()), None).await?;
    assert_eq!(gone.status, StatusCode::UNAUTHORIZED);
    // A user without `settings` cannot make keys.
    let viewer = app.signed_in("viewer", "Viewer").await?;
    let denied = app.call(Method::POST, "/api/api-keys", Some(&viewer), Some(mk("x", false))).await?;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    Ok(())
}

fn id_of(v: &Value) -> String {
    v["id"].as_str().unwrap_or_default().to_string()
}

/// `base` with `extra`'s keys merged over it.
fn merged(base: &Value, extra: Value) -> Value {
    let mut b = base.clone();
    if let (Some(o), Some(e)) = (b.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    b
}

/// D42 US payroll (ported from the web edition's `tests/api/us-payroll.test.ts`). 2026, biweekly,
/// Texas, SUTA 2.7% on the first $9,000:
///   A salary $52,000, single → gross 2,000.00; federal 52,000 − 8,600 = 43,400 →
///     1,240 + 12% × 23,500 = 4,060 / 26 = 156.15; net 1,690.85; cost 2,219.00
///   B hourly $25.50 × 80 h = 2,040.00, married jointly, Step 2 box → 4,184.80 / 26 = 160.95
#[tokio::test]
async fn us_payroll_settings_w4_and_preview_through_the_api() -> TestResult {
    let app = App::new().await?;
    let admin = app.signed_in("boss", "Admin").await?;
    let mut emp = Vec::new();
    for code in ["US-001", "US-002", "US-003"] {
        let body = json!({"employeeId": code, "name": format!("Employee {code}")});
        emp.push(id_of(&app.call(Method::POST, "/api/employees", Some(&admin), Some(body)).await?.body));
    }
    let tax_url = |i: usize| format!("/api/payroll/employees/{}/us-tax", emp[i]);
    let salaried = json!({"workState": "TX", "payType": "SALARY", "annualSalary": 52000});
    let settings = "/api/payroll/us/settings";

    // Settings: defaults, saving, null clears, refusals.
    let r = app.call(Method::GET, settings, Some(&admin), None).await?;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        r.body,
        json!({"country": "LK", "payFrequency": "BIWEEKLY", "periodsPerYear": 26, "state": null, "sutaRate": 0,
               "sutaWageBase": null, "futaCreditReduction": 0, "ein": null, "stateAccountNo": null, "taxYears": [2026]})
    );
    let body = json!({"country": "US", "state": "tx", "sutaRate": 2.7, "sutaWageBase": 9000,
                      "ein": "12-3456789", "stateAccountNo": " 99-123 "});
    let r = app.call(Method::PATCH, settings, Some(&admin), Some(body)).await?;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for (k, v) in [
        ("country", json!("US")),
        ("state", json!("TX")),
        ("sutaRate", json!(2.7)),
        ("sutaWageBase", json!(9000)),
        ("ein", json!("12-3456789")),
        ("stateAccountNo", json!("99-123")),
    ] {
        assert_eq!(r.body[k], v, "{k}");
    }
    let r = app.call(Method::PATCH, settings, Some(&admin), Some(json!({"stateAccountNo": null}))).await?;
    assert_eq!(r.body["stateAccountNo"], Value::Null);
    assert_eq!(r.body["sutaRate"], json!(2.7));
    for (body, message) in [
        (json!({"country": "UK"}), "country must be one of the following values: LK, US"),
        (json!({"state": "ZZ"}), "state must be a two-letter US state code"),
        (json!({"ein": "123456789"}), "ein must look like 12-3456789"),
        (json!({"sutaRate": 2.12345}), "sutaRate must have at most 4 decimal places"),
        (json!({"sutaRate": 101}), "sutaRate must not be greater than 100"),
        (json!({"futaCreditReduction": 6}), "futaCreditReduction must not be greater than 5.4"),
        (json!({"payFrequency": "DAILY"}), "payFrequency must be one of the following values"),
    ] {
        let r = app.call(Method::PATCH, settings, Some(&admin), Some(body.clone())).await?;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{body}");
        assert!(r.body["message"].to_string().contains(message), "{} lacks {message}", r.body["message"]);
    }

    // Employee pay and Form W-4.
    let r = app.call(Method::GET, &tax_url(0), Some(&admin), None).await?;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["configured"], json!(false));
    assert_eq!(r.body["workState"], json!("TX"));
    assert_eq!(r.body["w4Version"], json!("2020"));
    assert_eq!(r.body["filingStatus"], json!("SINGLE"));
    let r = app.call(Method::PUT, &tax_url(0), Some(&admin), Some(salaried.clone())).await?;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["configured"], json!(true));
    assert_eq!(r.body["annualSalary"], json!(52000));
    assert_eq!(r.body["pretax401k"], json!(0));
    let body = json!({"workState": "tx", "payType": "HOURLY", "hourlyRate": 25.5,
                      "filingStatus": "MARRIED_JOINTLY", "multipleJobs": true});
    let r = app.call(Method::PUT, &tax_url(1), Some(&admin), Some(body)).await?;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["workState"], json!("TX"));
    assert_eq!(r.body["hourlyRate"], json!(25.5));
    assert_eq!(r.body["multipleJobs"], json!(true));

    let bad = [
        (merged(&salaried, json!({"workState": "XX"})), None),
        (json!({"workState": "TX", "payType": "SALARY"}), Some("annualSalary is required for a salaried employee")),
        (json!({"workState": "TX", "payType": "HOURLY"}), Some("hourlyRate is required for an hourly employee")),
        (
            merged(&salaried, json!({"w4Version": "LEGACY", "filingStatus": "HEAD_OF_HOUSEHOLD"})),
            Some("Head of household is only on the 2020 and later Form W-4"),
        ),
        (merged(&salaried, json!({"allowances": 100})), None),
        (merged(&salaried, json!({"annualSalary": -1})), None),
    ];
    for (body, message) in bad {
        let r = app.call(Method::PUT, &tax_url(2), Some(&admin), Some(body.clone())).await?;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{body}");
        if let Some(message) = message {
            assert_eq!(r.body["message"], json!(message));
        }
    }
    assert_eq!(app.call(Method::GET, &tax_url(2), Some(&admin), None).await?.body["configured"], json!(false));
    let missing = "/api/payroll/employees/00000000-0000-4000-8000-000000000000/us-tax";
    assert_eq!(app.call(Method::GET, missing, Some(&admin), None).await?.status, StatusCode::NOT_FOUND);

    // Guards: 401 without a token, 403 without the payroll permission.
    assert_eq!(app.call(Method::GET, &tax_url(0), None, None).await?.status, StatusCode::UNAUTHORIZED);
    let role = json!({"name": "HR", "permissions": ["employees"]});
    app.call(Method::POST, "/api/roles", Some(&admin), Some(role)).await?;
    let hr = app.signed_in("hr1", "HR").await?;
    assert_eq!(app.call(Method::GET, &tax_url(0), Some(&hr), None).await?.status, StatusCode::FORBIDDEN);
    assert_eq!(app.call(Method::GET, settings, Some(&hr), None).await?.status, StatusCode::FORBIDDEN);
    let body = json!({"employeeId": emp[0]});
    let r = app.call(Method::POST, "/api/payroll/us/preview", Some(&hr), Some(body)).await?;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Paycheck preview.
    let base = json!({"payDate": "2026-11-06"});
    let preview =
        |extra: Value| app.call(Method::POST, "/api/payroll/us/preview", Some(&admin), Some(merged(&base, extra)));
    let r = preview(json!({"employeeId": emp[0]})).await?;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    assert_eq!(r.body["gross"], json!(2000));
    assert_eq!(r.body["periodsPerYear"], json!(26));
    assert_eq!(
        r.body["taxes"],
        json!({"incomeTaxWages": 2000, "federalIncomeTax": 156.15,
               "socialSecurityWages": 2000, "socialSecurityEmployee": 124, "socialSecurityEmployer": 124,
               "medicareWages": 2000, "medicareEmployee": 29, "additionalMedicare": 0, "medicareEmployer": 29,
               "futaWages": 2000, "futa": 12, "sutaWages": 2000, "suta": 54,
               "employeeTaxes": 309.15, "netPay": 1690.85, "employerTaxes": 219, "costToEmployer": 2219})
    );
    assert_eq!(r.body["notes"], json!(["State and local income tax are not included yet."]));

    let r = preview(json!({"employeeId": emp[1]})).await?;
    assert_eq!(r.body["message"], json!("hours is required for an hourly employee"));
    let r = preview(json!({"employeeId": emp[1], "hours": 80})).await?;
    assert_eq!(r.body["gross"], json!(2040));
    assert_eq!(r.body["taxes"]["federalIncomeTax"], json!(160.95));

    // 401(k) is still FICA wages, section 125 is not. Income tax wages 1,850 → 138.15.
    let body = merged(&salaried, json!({"pretax401k": 100, "pretaxSection125": 50}));
    app.call(Method::PUT, &tax_url(0), Some(&admin), Some(body)).await?;
    let t = preview(json!({"employeeId": emp[0]})).await?.body["taxes"].clone();
    for (k, v) in [
        ("incomeTaxWages", json!(1850)),
        ("federalIncomeTax", json!(138.15)),
        ("socialSecurityWages", json!(1950)),
        ("socialSecurityEmployee", json!(120.9)),
        ("medicareEmployee", json!(28.28)), // 28.275 half-up
        ("netPay", json!(1562.67)),
    ] {
        assert_eq!(t[k], v, "{k}");
    }
    app.call(Method::PUT, &tax_url(0), Some(&admin), Some(salaried.clone())).await?;

    // Year-to-date wages stop the capped taxes.
    let ytd = json!({"employeeId": emp[0], "ytdSocialSecurityWages": 184000, "ytdMedicareWages": 199000,
                     "ytdFutaWages": 7000, "ytdSutaWages": 8000});
    let t = preview(ytd).await?.body["taxes"].clone();
    for (k, v) in [
        ("socialSecurityWages", json!(500)),
        ("socialSecurityEmployee", json!(31)),
        ("additionalMedicare", json!(9)), // 0.9% of the 1,000 over 200,000
        ("futa", json!(0)),
        ("sutaWages", json!(1000)),
        ("suta", json!(27)),
    ] {
        assert_eq!(t[k], v, "{k}");
    }

    // Refusals.
    let r = preview(json!({"employeeId": emp[2]})).await?;
    assert_eq!(r.body["message"], json!("Add the US pay and Form W-4 details for Employee US-003 first"));
    let r = preview(json!({"employeeId": emp[0], "payDate": "2025-12-31"})).await?;
    assert_eq!(r.body["message"], json!("US payroll tax tables for 2025 are not set up yet"));
    assert_eq!(preview(json!({"employeeId": "nope"})).await?.status, StatusCode::BAD_REQUEST);
    assert_eq!(preview(json!({"employeeId": emp[0], "gross": -5})).await?.status, StatusCode::BAD_REQUEST);
    assert_eq!(preview(json!({"employeeId": emp[0], "gross": 1500})).await?.body["gross"], json!(1500));
    Ok(())
}
