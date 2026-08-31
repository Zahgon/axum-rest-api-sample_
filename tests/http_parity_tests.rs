use reqwest::{Client, StatusCode, header};
use serial_test::serial;
use uuid::Uuid;

pub mod common;
use common::{
    auth,
    constants::{
        API_PATH_AUTH, API_PATH_HEALTH, API_PATH_USERS, API_V1, TEST_ADMIN_PASSWORD_HASH,
        TEST_ADMIN_USERNAME,
    },
    helpers, test_app,
};

// Protocol level tests of the web layer.
// These tests pin the HTTP contract of the service: routing, method matching,
// request rejections and header handling, which are provided by the web framework.

#[tokio::test]
#[serial]
async fn method_not_allowed_test() {
    // Start API server.
    let app = test_app::run().await;

    // A known route requested with a method that is not routed.
    let url = helpers::build_url(API_V1, API_PATH_AUTH, "login");
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let allow = response
        .headers()
        .get(header::ALLOW)
        .expect("the response must advertise the allowed methods")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(allow.contains("POST"), "unexpected allow header: {}", allow);

    // A resource route requested with a method that is not routed.
    let url = helpers::build_url(API_V1, API_PATH_USERS, &Uuid::new_v4().to_string());
    let response = Client::new().patch(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    // A top level route requested with a method that is not routed.
    let url = format!("{}/head", helpers::config().service_http_addr());
    let response = Client::new().post(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn request_rejection_test() {
    // Start API server.
    let app = test_app::run().await;

    // Authenticate, the rejections below must be the only invalid part of the request.
    let tokens = auth::login(TEST_ADMIN_USERNAME, TEST_ADMIN_PASSWORD_HASH)
        .await
        .unwrap();
    let authorization = format!("Bearer {}", tokens.access_token);

    let url = helpers::build_path(API_V1, API_PATH_USERS);

    // A json payload sent without the content type header.
    let response = Client::new()
        .post(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .body(r#"{"username":"parity"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        response.text().await.unwrap(),
        "Expected request with `Content-Type: application/json`"
    );

    // A syntactically valid json payload that does not match the target type.
    let response = Client::new()
        .post(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body(r#"{"unexpected":"payload"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .starts_with("Failed to deserialize the JSON body into the target type:")
    );

    // A field of the wrong type: the message must name the field.
    //
    // The source framework's extractor deserializes through
    // `serde_path_to_error`, so the offending path is part of the message.
    // The case above is a *missing field*, whose path is empty and which
    // therefore carries no prefix -- it cannot show this, and the assertion
    // above only checks the prefix in any event. A mutation run and a
    // 55-request differential together found that every wrong-type rejection
    // had lost its path.
    let response = Client::new()
        .post(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body(r#"{"username":1}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        response.text().await.unwrap(),
        "Failed to deserialize the JSON body into the target type: \
username: invalid type: integer `1`, expected a string at line 1 column 13"
    );

    // A nested path is spelled the same way, with the index in brackets.
    let response = Client::new()
        .post(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body("[1,2]")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .contains("[0]: invalid type: integer `1`")
    );

    // A malformed json payload.
    let response = Client::new()
        .post(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body("{not-a-json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .starts_with("Failed to parse the request body as JSON:")
    );

    // A json payload that exceeds the buffering limit of the web framework.
    let response = Client::new()
        .post(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .header(header::CONTENT_TYPE, "application/json")
        .body(format!(
            r#"{{"username":"{}"}}"#,
            "x".repeat(3 * 1024 * 1024)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        response.text().await.unwrap(),
        "Failed to buffer the request body: length limit exceeded"
    );

    // A path parameter that cannot be parsed into the target type.
    let url = helpers::build_url(API_V1, API_PATH_USERS, "not-an-uuid");
    let response = Client::new()
        .get(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .text()
            .await
            .unwrap()
            .starts_with("Invalid URL: Cannot parse `id` with value `not-an-uuid`:")
    );

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn route_matching_test() {
    // Start API server.
    let app = test_app::run().await;

    let tokens = auth::login(TEST_ADMIN_USERNAME, TEST_ADMIN_PASSWORD_HASH)
        .await
        .unwrap();
    let authorization = format!("Bearer {}", tokens.access_token);

    // A collection route does not match the trailing slash form.
    let url = format!("{}/", helpers::build_path(API_V1, API_PATH_USERS));
    let response = Client::new()
        .get(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // An unknown route is handled by the fallback handler, without a body.
    let url = helpers::build_path(API_V1, "unknown");
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(response.text().await.unwrap().is_empty());

    // The health route is served without the api version validation.
    let url = helpers::build_path("v9", API_PATH_HEALTH);
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // The routes guarded by the api version reject an unknown version.
    let url = helpers::build_path("v9", API_PATH_USERS);
    let response = Client::new()
        .get(url.as_str())
        .header(header::AUTHORIZATION, authorization.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.text().await.unwrap();
    let api_error = serde_json::from_str::<serde_json::Value>(&body).unwrap();
    assert_eq!(api_error["errors"][0]["code"], "api_version_error");
    assert_eq!(api_error["errors"][0]["message"], "unknown version: v9");

    // The any route is served for any method.
    let url = format!("{}/any?q=1", helpers::config().service_http_addr());
    let response = Client::new().post(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn head_and_get_request_test() {
    // Start API server.
    let app = test_app::run().await;

    let url = format!("{}/head", helpers::config().service_http_addr());

    // The `GET` request is served with a body.
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-some-header").unwrap(),
        "header from GET"
    );
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(response.text().await.unwrap(), "body from GET");

    // The `HEAD` request is served with headers only.
    let response = Client::new().head(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-some-header").unwrap(),
        "header from HEAD"
    );
    assert!(response.text().await.unwrap().is_empty());

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn authorization_header_test() {
    // Start API server.
    let app = test_app::run().await;

    let url = helpers::config().service_http_addr();

    // A request without the authorization header.
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // A request with an unsupported authorization scheme.
    for authorization in [
        "Basic YWRtaW46YWRtaW4=",
        "Bearer",
        "Bearer ",
        "not-a-scheme",
    ] {
        let response = Client::new()
            .get(url.as_str())
            .header(header::AUTHORIZATION, authorization)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "unexpected status for the authorization header: {}",
            authorization
        );
        let body = response.text().await.unwrap();
        let api_error = serde_json::from_str::<serde_json::Value>(&body).unwrap();
        assert_eq!(
            api_error["errors"][0]["code"],
            "authentication_wrong_credentials"
        );
    }

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn head_request_routing_test() {
    // Start API server.
    let app = test_app::run().await;

    // The routes served with `GET` are served with `HEAD` as well,
    // the request reaches the handler and its extractors.
    let url = helpers::build_path(API_V1, API_PATH_HEALTH);
    let response = Client::new().head(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let url = helpers::config().service_http_addr();
    let response = Client::new().head(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let url = helpers::build_path(API_V1, API_PATH_USERS);
    let response = Client::new().head(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let url = helpers::build_url(API_V1, API_PATH_USERS, &Uuid::new_v4().to_string());
    let response = Client::new().head(url.as_str()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn cors_header_test() {
    // Start API server.
    let app = test_app::run().await;

    let url = helpers::build_path(API_V1, API_PATH_HEALTH);

    // Any origin is allowed, the header is served regardless of the request origin.
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*"
    );
    // The value, not merely its presence: `tower-http`'s CorsLayer names all
    // three request headers it varies on, and the reconstructed middleware has
    // to name the same three. Asserting `is_some()` let the header shrink to
    // `origin` alone with the suite still green -- a mutation run found it.
    assert_eq!(
        response.headers().get(header::VARY).unwrap(),
        "origin, access-control-request-method, access-control-request-headers"
    );

    let response = Client::new()
        .get(url.as_str())
        .header(header::ORIGIN, "https://example.com")
        .send()
        .await
        .unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*"
    );

    // The error responses carry the header as well.
    let url = helpers::build_path(API_V1, "unknown");
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*"
    );

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn options_request_test() {
    // Start API server.
    let app = test_app::run().await;

    // Every `OPTIONS` request is answered by the cors layer,
    // with an empty body and the methods of the matched route.
    let health = helpers::build_path(API_V1, API_PATH_HEALTH);
    let users = helpers::build_path(API_V1, API_PATH_USERS);
    let user = helpers::build_url(API_V1, API_PATH_USERS, &Uuid::new_v4().to_string());
    let login = helpers::build_url(API_V1, API_PATH_AUTH, "login");
    let root = helpers::config().service_http_addr();
    let unknown = helpers::build_path(API_V1, "unknown");

    for (url, allow) in [
        (root.as_str(), Some("GET,HEAD")),
        (health.as_str(), Some("GET,HEAD")),
        (users.as_str(), Some("GET,HEAD,POST")),
        (user.as_str(), Some("GET,HEAD,PUT,DELETE")),
        (login.as_str(), Some("POST")),
        (unknown.as_str(), None),
    ] {
        let response = Client::new()
            .request(reqwest::Method::OPTIONS, url)
            .header(header::ORIGIN, "https://example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "unexpected status: {url}"
        );
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert_eq!(
            response
                .headers()
                .get(header::ALLOW)
                .map(|value| value.to_str().unwrap().to_owned()),
            allow.map(ToOwned::to_owned),
            "unexpected allow header: {url}"
        );
        assert!(response.text().await.unwrap().is_empty());
    }

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}

#[tokio::test]
#[serial]
async fn truncated_request_body_test() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Start API server.
    let app = test_app::run().await;

    // A request that announces more body than it sends, then closes its write
    // half. The body stream fails mid-read, which is a body-buffering failure
    // and not a protocol error: `400` with the original framework's message.
    let addr = helpers::config().service_socket_addr();
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            b"POST /v1/auth/login HTTP/1.1\r\nHost: localhost\r\n\
              Content-Type: application/json\r\nContent-Length: 500\r\n\
              Connection: close\r\n\r\n{}",
        )
        .await
        .unwrap();
    stream.shutdown().await.unwrap();

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8(response).unwrap();

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request\r\n"),
        "unexpected status line: {response}"
    );
    assert!(
        response
            .to_ascii_lowercase()
            .contains("content-type: text/plain; charset=utf-8"),
        "unexpected content type: {response}"
    );
    let body = response.split_once("\r\n\r\n").unwrap().1;
    assert_eq!(
        body,
        "Failed to buffer the request body: error reading a body from connection"
    );

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}
