use actix_web::{HttpResponse, http::StatusCode, web};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::types::Uuid;

use crate::{
    api::extractors,
    api::{APIError, APIErrorCode, APIErrorEntry, APIErrorKind, version::APIVersion},
    application::{
        repository::user_repo,
        security::{
            auth::{self, AuthError, JwtTokens},
            jwt::{AccessClaims, ClaimsMethods, RefreshClaims},
        },
        service::token_service,
        state::AppState,
    },
};

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginUser {
    username: String,
    password_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RevokeUser {
    user_id: Uuid,
}

#[tracing::instrument(level = tracing::Level::TRACE, name = "login", skip_all, fields(username=login.username))]
pub async fn login_handler(
    api_version: APIVersion,
    state: web::Data<AppState>,
    extractors::Json(login): extractors::Json<LoginUser>,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    if let Ok(user) = user_repo::get_by_username(&login.username, &state).await {
        if user.active && user.password_hash == login.password_hash {
            tracing::trace!("access granted, user: {}", user.id);
            let tokens = auth::generate_tokens(user, &state.config);
            let response = tokens_to_response(tokens);
            return Ok(response);
        }
    }

    tracing::error!("access denied: {:#?}", login);
    Err(AuthError::WrongCredentials)?
}

pub async fn logout_handler(
    api_version: APIVersion,
    state: web::Data<AppState>,
    refresh_claims: RefreshClaims,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    tracing::trace!("refresh_claims: {:?}", refresh_claims);
    auth::logout(refresh_claims, state).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn refresh_handler(
    api_version: APIVersion,
    state: web::Data<AppState>,
    refresh_claims: RefreshClaims,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    let new_tokens = auth::refresh(refresh_claims, state).await?;
    Ok(tokens_to_response(new_tokens))
}

// Revoke all issued tokens until now.
pub async fn revoke_all_handler(
    api_version: APIVersion,
    state: web::Data<AppState>,
    access_claims: AccessClaims,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    access_claims.validate_role_admin()?;
    token_service::revoke_global(&state).await?;
    Ok(HttpResponse::Ok().finish())
}

// Revoke tokens issued to user until now.
pub async fn revoke_user_handler(
    api_version: APIVersion,
    state: web::Data<AppState>,
    access_claims: AccessClaims,
    extractors::Json(revoke_user): extractors::Json<RevokeUser>,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    if access_claims.sub != revoke_user.user_id.to_string() {
        // Only admin can revoke tokens of other users.
        access_claims.validate_role_admin()?;
    }
    tracing::trace!("revoke_user: {:?}", revoke_user);
    token_service::revoke_user_tokens(&revoke_user.user_id.to_string(), &state).await?;
    Ok(HttpResponse::Ok().finish())
}

pub async fn cleanup_handler(
    api_version: APIVersion,
    state: web::Data<AppState>,
    access_claims: AccessClaims,
) -> Result<HttpResponse, APIError> {
    let state = state.into_inner();
    tracing::trace!("api version: {}", api_version);
    access_claims.validate_role_admin()?;
    tracing::trace!("authentication details: {:#?}", access_claims);
    let deleted = auth::cleanup_revoked_and_expired(&access_claims, &state).await?;
    let json = json!({
        "deleted_tokens": deleted,
    });
    Ok(HttpResponse::Ok().json(json))
}

fn tokens_to_response(jwt_tokens: JwtTokens) -> HttpResponse {
    let json = json!({
        "access_token": jwt_tokens.access_token,
        "refresh_token": jwt_tokens.refresh_token,
        "token_type": "Bearer"
    });

    tracing::trace!("JWT: generated response {:#?}", json);
    HttpResponse::Ok().json(json)
}

impl From<AuthError> for APIError {
    fn from(auth_error: AuthError) -> Self {
        let (status_code, code) = match auth_error {
            AuthError::WrongCredentials => (
                StatusCode::UNAUTHORIZED,
                APIErrorCode::AuthenticationWrongCredentials,
            ),
            AuthError::MissingCredentials => (
                StatusCode::BAD_REQUEST,
                APIErrorCode::AuthenticationMissingCredentials,
            ),
            AuthError::TokenCreationError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                APIErrorCode::AuthenticationTokenCreationError,
            ),
            AuthError::InvalidToken => (
                StatusCode::BAD_REQUEST,
                APIErrorCode::AuthenticationInvalidToken,
            ),
            AuthError::Forbidden => (StatusCode::FORBIDDEN, APIErrorCode::AuthenticationForbidden),
            AuthError::RevokedTokensInactive => (
                StatusCode::BAD_REQUEST,
                APIErrorCode::AuthenticationRevokedTokensInactive,
            ),
            AuthError::RedisError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, APIErrorCode::RedisError)
            }
            AuthError::SQLxError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                APIErrorCode::DatabaseError,
            ),
        };

        let error = APIErrorEntry::new(&auth_error.to_string())
            .code(code)
            .kind(APIErrorKind::AuthenticationError);

        Self {
            status: status_code.as_u16(),
            errors: vec![error],
        }
    }
}
