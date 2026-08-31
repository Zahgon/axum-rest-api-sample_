use actix_web::{HttpResponse, http::StatusCode, web};
use sqlx::types::Uuid;
use thiserror::Error;

use crate::{
    api::extractors,
    api::{
        APIError, APIErrorCode, APIErrorEntry, APIErrorKind,
        error::API_DOCUMENT_URL,
        version::{self, APIVersion},
    },
    application::{
        repository::user_repo,
        security::jwt::{AccessClaims, ClaimsMethods},
        state::AppState,
    },
    domain::models::user::User,
};

pub async fn list_users_handler(
    api_version: APIVersion,
    access_claims: AccessClaims,
    state: web::Data<AppState>,
) -> Result<web::Json<Vec<User>>, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    access_claims.validate_role_admin()?;
    let users = user_repo::list(&state).await?;
    Ok(web::Json(users))
}

pub async fn add_user_handler(
    api_version: APIVersion,
    access_claims: AccessClaims,
    state: web::Data<AppState>,
    extractors::Json(user): extractors::Json<User>,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    access_claims.validate_role_admin()?;
    let user = user_repo::add(user, &state).await?;
    Ok(HttpResponse::Created().json(user))
}

pub async fn get_user_handler(
    access_claims: AccessClaims,
    path: web::Path<(String, Uuid)>,
    state: web::Data<AppState>,
) -> Result<web::Json<User>, APIError> {
    let state = state.into_inner();
    let (version, id) = path.into_inner();
    let api_version: APIVersion = version::parse_version(&version)?;
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    tracing::trace!("id: {}", id);
    access_claims.validate_role_admin()?;
    let user = user_repo::get_by_id(id, &state)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                let user_error = UserError::UserNotFound(id);
                (user_error.status_code(), APIErrorEntry::from(user_error)).into()
            }
            _ => APIError::from(e),
        })?;

    Ok(web::Json(user))
}

pub async fn update_user_handler(
    access_claims: AccessClaims,
    path: web::Path<(String, Uuid)>,
    state: web::Data<AppState>,
    extractors::Json(user): extractors::Json<User>,
) -> Result<web::Json<User>, APIError> {
    let state = state.into_inner();
    let (version, id) = path.into_inner();
    let api_version: APIVersion = version::parse_version(&version)?;
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    tracing::trace!("id: {}", id);
    access_claims.validate_role_admin()?;
    let user = user_repo::update(user, &state).await?;
    Ok(web::Json(user))
}

pub async fn delete_user_handler(
    access_claims: AccessClaims,
    path: web::Path<(String, Uuid)>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    let (version, id) = path.into_inner();
    let api_version: APIVersion = version::parse_version(&version)?;
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("authentication details: {:#?}", access_claims);
    tracing::trace!("id: {}", id);
    access_claims.validate_role_admin()?;
    if user_repo::delete(id, &state).await? {
        Ok(HttpResponse::Ok().finish())
    } else {
        Err(StatusCode::NOT_FOUND)?
    }
}

#[derive(Debug, Error)]
enum UserError {
    #[error("user not found: {0}")]
    UserNotFound(Uuid),
}

impl UserError {
    const fn status_code(&self) -> StatusCode {
        match self {
            Self::UserNotFound(_) => StatusCode::NOT_FOUND,
        }
    }
}

impl From<UserError> for APIErrorEntry {
    fn from(user_error: UserError) -> Self {
        let message = user_error.to_string();
        match user_error {
            UserError::UserNotFound(user_id) => Self::new(&message)
                .code(APIErrorCode::UserNotFound)
                .kind(APIErrorKind::ResourceNotFound)
                .description(&format!("user with the ID '{}' does not exist in our records", user_id))
                .detail(serde_json::json!({"user_id": user_id}))
                .reason("must be an existing user")
                .instance(&format!("/api/v1/users/{}", user_id))
                .trace_id()
                .help(&format!("please check if the user ID is correct or refer to our documentation at {}#errors for more information", API_DOCUMENT_URL))
                .doc_url()
        }
    }
}
