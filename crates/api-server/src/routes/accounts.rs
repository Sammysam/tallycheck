//! Reference `accounts.controller.ts`. Reading the chart is open to any signed-in user (the
//! invoice, bill and expense forms need it); changing it needs the `accounts` permission.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized, CurrentUser};
use crate::state::AppState;
use crate::validate::{self, AccountDto};
use app_core::{AppError, AppResult};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use ledger::accounts::{self, CreateAccount, UpdateAccount};
use ledger::{Account, AccountType};

fn account_type(s: Option<&str>) -> AppResult<Option<AccountType>> {
    match s {
        None => Ok(None),
        Some(t) => AccountType::parse(t).map(Some).ok_or_else(|| AppError::internal("validated type did not parse")),
    }
}

pub async fn list(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Json<Vec<Account>>> {
    Ok(Json(accounts::find_all(&state.db).await?))
}

pub async fn create(
    State(state): State<AppState>,
    _auth: Authorized<perm::Accounts>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Account>)> {
    let dto: AccountDto = validate::create_account(body.object())?;
    let account = accounts::create(
        &state.db,
        CreateAccount {
            code: dto.code.unwrap_or_default(),
            name: dto.name.unwrap_or_default(),
            account_type: account_type(dto.account_type.as_deref())?
                .ok_or_else(|| AppError::internal("validated type missing"))?,
            subtype: dto.subtype.flatten(),
            description: dto.description.flatten(),
            parent_id: dto.parent_id.flatten(),
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(account)))
}

pub async fn find_one(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<Account>> {
    let mut conn = state.db.read().acquire().await?;
    Ok(Json(accounts::find_one(&mut conn, &id).await?))
}

/// `null` for code, name or type is ignored (the reference crashed on the NOT NULL column);
/// `null` for subtype, description or parentId clears the field, as in the reference.
pub async fn update(
    State(state): State<AppState>,
    _auth: Authorized<perm::Accounts>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Account>> {
    let dto = validate::update_account(body.object())?;
    let account = accounts::update(
        &state.db,
        &id,
        UpdateAccount {
            code: dto.code,
            name: dto.name,
            account_type: account_type(dto.account_type.as_deref())?,
            subtype: dto.subtype,
            description: dto.description,
            parent_id: dto.parent_id,
        },
    )
    .await?;
    Ok(Json(account))
}

pub async fn remove(
    State(state): State<AppState>,
    _auth: Authorized<perm::Accounts>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    accounts::remove(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn seed(State(state): State<AppState>, _auth: Authorized<perm::Accounts>) -> AppResult<StatusCode> {
    accounts::seed(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}
