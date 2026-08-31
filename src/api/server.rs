use std::{collections::HashMap, time::SystemTime};

use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer,
    body::{EitherBody, MessageBody},
    dev::{Server, ServiceRequest, ServiceResponse},
    error::{InternalError, PathError},
    http::{
        Method, StatusCode,
        header::{self, ContentType, HeaderMap, HeaderValue},
    },
    middleware::{Next, from_fn},
    web,
};
use chrono::Utc;
use serde_json::json;
use tokio::signal::{
    self,
    unix::{self, SignalKind},
};

use crate::{
    api::{
        error::APIError,
        routes::{account_routes, auth_routes, get_or_head, transaction_routes, user_routes},
    },
    application::{security::jwt::AccessClaims, state::SharedState},
};

pub async fn start(state: SharedState) {
    // Build the API service.
    let server = build(state);

    // Handle the graceful shutdown on `SIGINT` (Ctrl+C) and `SIGTERM`.
    let handle = server.handle();
    tokio::spawn(async move {
        shutdown_signal().await;
        handle.stop(true).await;
    });

    // Start the API service.
    server.await.unwrap();

    tracing::info!("server shutdown successfully.");
}

pub fn build(state: SharedState) -> Server {
    // Build the shared application state.
    let app_state = web::Data::from(state.clone());

    // Build the listening address.
    let addr = state.config.service_socket_addr();

    // Build the API service.
    let server = HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .app_data(path_config())
            .service(web::resource("/").route(get_or_head().to(root_handler)))
            .service(web::resource("/head").route(get_or_head().to(head_request_handler)))
            .service(web::resource("/any").route(web::route().to(any_request_handler)))
            .service(web::resource("/{version}/health").route(get_or_head().to(health_handler)))
            .service(web::resource("/{version}/version").route(get_or_head().to(version_handler)))
            // Scoping authentication routes.
            .service(web::scope("/{version}/auth").configure(auth_routes::routes))
            // Scoping user routes.
            .service(web::scope("/{version}/users").configure(user_routes::routes))
            // Scoping account routes.
            .service(web::scope("/{version}/accounts").configure(account_routes::routes))
            // Scoping transaction routes.
            .service(web::scope("/{version}/transactions").configure(transaction_routes::routes))
            // Add a default service for handling routes to unknown paths.
            .default_service(web::route().to(error_404_handler))
            .wrap(from_fn(cors_middleware))
            .wrap(from_fn(logging_middleware))
    })
    .disable_signals()
    .bind(&addr)
    .unwrap()
    .run();

    tracing::info!("listening on {}", addr);

    server
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        unix::signal(SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("received termination signal, shutting down...");
}

// Configures the path parameters extractor.
// Rejects the requests carrying invalid path parameters with `400`.
// The typed path parameter of the API is `{id}`, it is named in the rejection message.
fn path_config() -> web::PathConfig {
    web::PathConfig::default().error_handler(|error, request| {
        let message = match (&error, request.match_info().get("id")) {
            (PathError::Deserialize(reason), Some(value)) => {
                format!("Invalid URL: Cannot parse `id` with value `{value}`: {reason}")
            }
            _ => error.to_string(),
        };
        let response = HttpResponse::BadRequest()
            .content_type(ContentType::plaintext())
            .body(message);
        InternalError::from_response(error, response).into()
    })
}

// Allows cross-origin requests from any origin.
// Every response carries a wildcard `access-control-allow-origin` header,
// and every `OPTIONS` request is answered with `200 OK` and an empty body,
// keeping the `allow` header of the requested path when the path is known.
pub async fn cors_middleware<B>(
    request: ServiceRequest,
    next: Next<B>,
) -> Result<ServiceResponse<EitherBody<B>>, actix_web::Error>
where
    B: MessageBody,
{
    let preflight = request.method() == Method::OPTIONS;
    let response = next.call(request).await?;

    let mut response = if preflight {
        let allow = response.headers().get(header::ALLOW).cloned();
        let mut builder = HttpResponse::Ok();
        if let Some(allow) = allow {
            builder.insert_header((header::ALLOW, allow));
        }
        response
            .into_response(builder.finish())
            .map_into_right_body()
    } else {
        response.map_into_left_body()
    };

    let headers = response.headers_mut();
    set_cors_headers(headers);
    compact_allow_header(headers);

    Ok(response)
}

fn compact_allow_header(headers: &mut HeaderMap) {
    let Some(allow) = headers.get(header::ALLOW).map(compact_header_list) else {
        return;
    };
    headers.insert(header::ALLOW, allow);
}

fn set_cors_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::VARY,
        HeaderValue::from_static(
            "origin, access-control-request-method, access-control-request-headers",
        ),
    );
}

// Removes the whitespace of a comma separated header value, such as `GET, HEAD`.
fn compact_header_list(value: &HeaderValue) -> HeaderValue {
    value.to_str().map_or_else(
        |_| value.clone(),
        |list| HeaderValue::from_str(&list.replace(", ", ",")).unwrap_or_else(|_| value.clone()),
    )
}

#[tracing::instrument(level = tracing::Level::TRACE, name = "actix", skip_all, fields(method=request.method().to_string(), uri=request.uri().to_string()))]
pub async fn logging_middleware(
    request: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, actix_web::Error> {
    tracing::trace!(
        "received a {} request to {}",
        request.method(),
        request.uri()
    );
    next.call(request).await
}

// Root handler.
pub async fn root_handler(access_claims: AccessClaims) -> Result<HttpResponse, APIError> {
    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!("authentication details: {:#?}", access_claims);
        let timestamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .as_secs();
        tracing::trace!("timestamp, std::time {}", timestamp);
        tracing::trace!("timestamp, chrono::Utc {}", Utc::now().timestamp() as usize);
    }
    Ok(HttpResponse::Ok().json(json!({"message": "Hello from Axum-Web!"})))
}

// Health request handler.
pub async fn health_handler() -> Result<HttpResponse, APIError> {
    Ok(HttpResponse::Ok().json(json!({"status": "healthy"})))
}

// Version request handler.
pub async fn version_handler() -> Result<HttpResponse, APIError> {
    let result = json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
    });
    Ok(HttpResponse::Ok().json(result))
}

// A sample head request handler.
// Using HEAD requests makes sense if processing (computing) the response body is costly.
pub async fn head_request_handler(request: HttpRequest) -> HttpResponse {
    if request.method() == Method::HEAD {
        tracing::debug!("HEAD method found");
        return HttpResponse::Ok()
            .insert_header(("x-some-header", "header from HEAD"))
            .finish();
    }
    HttpResponse::Ok()
        .insert_header(("x-some-header", "header from GET"))
        .content_type(ContentType::plaintext())
        .body("body from GET")
}

// A sample any request handler.
pub async fn any_request_handler(
    request: HttpRequest,
    web::Query(params): web::Query<HashMap<String, String>>,
) -> HttpResponse {
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!("method: {:?}", request.method());
        tracing::debug!("headers: {:?}", request.headers());
        tracing::debug!("params: {:?}", params);
        tracing::debug!("request: {:?}", request);
    }
    HttpResponse::Ok().finish()
}

// 404 handler.
pub async fn error_404_handler(request: HttpRequest) -> HttpResponse {
    tracing::error!("route not found: {:?}", request);
    HttpResponse::NotFound().finish()
}
