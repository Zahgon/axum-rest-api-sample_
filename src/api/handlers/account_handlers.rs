use actix_web::{HttpResponse, http::StatusCode, web};
use sqlx::types::Uuid;

use crate::{
    api::extractors,
    api::{
        APIError,
        version::{self, APIVersion},
    },
    application::{
        repository::account_repo,
        security::jwt::{AccessClaims, ClaimsMethods},
        state::AppState,
    },
    domain::models::account::Account,
};

pub async fn list_accounts_handler(
    api_version: APIVersion,
    access_claims: AccessClaims,
    state: web::Data<AppState>,
) -> Result<web::Json<Vec<Account>>, APIError> {
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);

    access_claims.validate_role_admin()?;

    let mut connection = state.db_pool.acquire().await?;
    let accounts = account_repo::list(&mut connection).await?;
    Ok(web::Json(accounts))
}

pub async fn add_account_handler(
    api_version: APIVersion,
    access_claims: AccessClaims,
    state: web::Data<AppState>,
    extractors::Json(account): extractors::Json<Account>,
) -> Result<HttpResponse, APIError> {
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);

    access_claims.validate_role_admin()?;

    let mut connection = state.db_pool.acquire().await?;
    let account = account_repo::add(account, &mut connection).await?;
    Ok(HttpResponse::Created().json(account))
}

pub async fn get_account_handler(
    access_claims: AccessClaims,
    path: web::Path<(String, Uuid)>,
    state: web::Data<AppState>,
) -> Result<web::Json<Account>, APIError> {
    let (version, id) = path.into_inner();
    let api_version: APIVersion = version::parse_version(&version)?;
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    tracing::trace!("id: {}", id);

    access_claims.validate_role_admin()?;

    let mut connection = state.db_pool.acquire().await?;
    let account = account_repo::get_by_id(id, &mut connection).await?;
    Ok(web::Json(account))
}

pub async fn update_account_handler(
    access_claims: AccessClaims,
    path: web::Path<(String, Uuid)>,
    state: web::Data<AppState>,
    extractors::Json(account): extractors::Json<Account>,
) -> Result<web::Json<Account>, APIError> {
    let (version, id) = path.into_inner();
    let api_version: APIVersion = version::parse_version(&version)?;
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    tracing::trace!("id: {}", id);
    tracing::trace!("account: {:?}", account);
    access_claims.validate_role_admin()?;

    let mut connection = state.db_pool.acquire().await?;
    let account = account_repo::update(account, &mut connection).await?;
    Ok(web::Json(account))
}

pub async fn delete_account_handler(
    access_claims: AccessClaims,
    path: web::Path<(String, Uuid)>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, APIError> {
    let (version, id) = path.into_inner();
    let api_version: APIVersion = version::parse_version(&version)?;
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    tracing::trace!("id: {}", id);
    access_claims.validate_role_admin()?;

    let mut connection = state.db_pool.acquire().await.unwrap();
    if account_repo::delete(id, &mut connection).await? {
        Ok(HttpResponse::Ok().finish())
    } else {
        Err(StatusCode::NOT_FOUND)?
    }
}
