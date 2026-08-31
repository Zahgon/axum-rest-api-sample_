use std::{future::Future, pin::Pin};

use actix_web::{FromRequest, HttpRequest, dev::Payload, http::StatusCode, http::header, web};

use crate::{
    api::APIError,
    application::{
        security::{
            auth::{self, AuthError},
            jwt::{AccessClaims, ClaimsMethods, RefreshClaims, decode_token},
        },
        state::AppState,
    },
};

type ClaimsFuture<T> = Pin<Box<dyn Future<Output = Result<T, APIError>>>>;

impl FromRequest for AccessClaims {
    type Error = APIError;
    type Future = ClaimsFuture<Self>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let request = req.clone();
        Box::pin(async move { decode_token_from_request(&request).await })
    }
}

impl FromRequest for RefreshClaims {
    type Error = APIError;
    type Future = ClaimsFuture<Self>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let request = req.clone();
        Box::pin(async move { decode_token_from_request(&request).await })
    }
}

async fn decode_token_from_request<T>(request: &HttpRequest) -> Result<T, APIError>
where
    T: for<'de> serde::Deserialize<'de> + std::fmt::Debug + ClaimsMethods + Sync + Send,
{
    // Extract the token from the authorization header.
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|header_value| header_value.to_str().ok())
        .and_then(parse_bearer_token)
        .ok_or_else(|| {
            tracing::error!("Invalid authorization header");
            AuthError::WrongCredentials
        })?;

    // Take the state from the application data.
    let state = request
        .app_data::<web::Data<AppState>>()
        .ok_or_else(|| {
            tracing::error!("Could not extract the application state");
            APIError::from(StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .clone()
        .into_inner();

    // Decode the token.
    let claims = decode_token::<T>(token, &state.config)?;

    // Check for revoked tokens if enabled by configuration.
    if state.config.jwt_enable_revoked_tokens {
        auth::validate_revoked(&claims, &state).await?
    }
    Ok(claims)
}

/// Parses the `Bearer` credentials of the authorization header value.
fn parse_bearer_token(header_value: &str) -> Option<&str> {
    let (scheme, token) = header_value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() {
        return None;
    }
    Some(token.trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;

    #[tokio::test]
    async fn missing_application_state_test() {
        let request = TestRequest::default()
            .insert_header((header::AUTHORIZATION, "Bearer token"))
            .to_http_request();

        let error = decode_token_from_request::<AccessClaims>(&request)
            .await
            .expect_err("the application state is not registered");

        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR.as_u16());
    }
}

/// `Json<T>`, deserialized the way the original framework's extractor did.
///
/// The source framework's JSON extractor runs the body through
/// `serde_path_to_error`, so a type mismatch names the offending field:
///
/// ```text
/// Failed to deserialize the JSON body into the target type: username: invalid type: integer `1`, expected a string at line 1 column 13
/// ```
///
/// `web::Json` reports the same serde error without the `username: ` prefix, so
/// every wrong-type rejection on every JSON endpoint lost the path. The status
/// codes and the surrounding wording were already reproduced in
/// `server.rs::json_config`; this restores the part of the message that names
/// what was actually wrong.
///
/// The limits and the 415/413/400/422 split match `json_config` exactly, and
/// that handler stays in place for any extraction that does not come through
/// here.
pub struct Json<T>(pub T);

/// The source framework's default request-body limit, and `web::JsonConfig`'s.
const JSON_PAYLOAD_LIMIT: usize = 2 * 1024 * 1024;

impl<T> FromRequest for Json<T>
where
    T: serde::de::DeserializeOwned + 'static,
{
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, actix_web::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let is_json = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                let media = value.split(';').next().unwrap_or("").trim();
                media.eq_ignore_ascii_case("application/json") || media.ends_with("+json")
            })
            .unwrap_or(false);

        let mut payload = payload.take();

        Box::pin(async move {
            if !is_json {
                return Err(json_rejection(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "Expected request with `Content-Type: application/json`".to_string(),
                ));
            }

            let mut body = actix_web::web::BytesMut::new();
            while let Some(chunk) = futures_util::StreamExt::next(&mut payload).await {
                // A payload that fails mid-stream — a `Content-Length` the client
                // never finished sending, a broken connection — is a body-buffering
                // failure, not a protocol error. The original framework reports every
                // one of them with this single message and a 400; actix surfaces its
                // own `PayloadError` text instead, which is the only part of the
                // rejection contract that differed.
                let chunk = chunk.map_err(|_| {
                    json_rejection(
                        StatusCode::BAD_REQUEST,
                        "Failed to buffer the request body: error reading a body from connection"
                            .to_string(),
                    )
                })?;
                if body.len() + chunk.len() > JSON_PAYLOAD_LIMIT {
                    return Err(json_rejection(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Failed to buffer the request body: length limit exceeded".to_string(),
                    ));
                }
                body.extend_from_slice(&chunk);
            }

            let deserializer = &mut serde_json::Deserializer::from_slice(&body);
            match serde_path_to_error::deserialize::<_, T>(deserializer) {
                Ok(value) => Ok(Self(value)),
                Err(error) => {
                    let path = error.path().to_string();
                    let inner = error.into_inner();
                    if inner.classify() == serde_json::error::Category::Data {
                        // The path is `.` when the failure is the whole body
                        // rather than one of its fields; the original prints no
                        // prefix in that case.
                        let detail = if path.is_empty() || path == "." {
                            inner.to_string()
                        } else {
                            format!("{path}: {inner}")
                        };
                        Err(json_rejection(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            format!(
                                "Failed to deserialize the JSON body into the target type: {detail}"
                            ),
                        ))
                    } else {
                        Err(json_rejection(
                            StatusCode::BAD_REQUEST,
                            format!("Failed to parse the request body as JSON: {inner}"),
                        ))
                    }
                }
            }
        })
    }
}

fn json_rejection(status: StatusCode, message: String) -> actix_web::Error {
    let response = actix_web::HttpResponse::build(status)
        .content_type(actix_web::http::header::ContentType::plaintext())
        .body(message.clone());
    actix_web::error::InternalError::from_response(message, response).into()
}
