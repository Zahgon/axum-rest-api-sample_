use reqwest::StatusCode;
use serial_test::serial;

pub mod common;
use common::{
    constants::{API_PATH_HEALTH, API_V1},
    helpers,
    tcp_fetch::tcp_fetch,
    test_app,
};

#[tokio::test]
#[serial]
async fn health_test() {
    // Start API server.
    let app = test_app::run().await;

    let url = helpers::build_path(API_V1, API_PATH_HEALTH);

    // Fetch using `reqwest`.
    let response = reqwest::get(url.as_str()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "healthy");

    // Fetch using a raw `TCP` connection.
    let body = tcp_fetch(url.as_str()).await.unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "healthy");

    // Stop the API server and drop the test database.
    app.drop().await.unwrap();
}
