use std::{sync::Arc, time::Duration};

use actix_web::dev::ServerHandle;
use reqwest::StatusCode;
use tokio::{sync::Mutex, time::Instant};

use axum_web::{
    api,
    application::{config, state::AppState},
    infrastructure::{
        database::{Database, DatabaseError, TestDatabase},
        redis,
    },
};

use crate::common::{
    constants::{API_PATH_HEALTH, API_V1},
    helpers,
};

// Handle of the running test application.
// Stopping the server on teardown releases the listening socket
// for the next test in the same test binary.
#[must_use]
pub struct TestApp {
    server: ServerHandle,
    database: TestDatabase,
}

impl TestApp {
    pub async fn drop(self) -> Result<(), DatabaseError> {
        self.server.stop(true).await;
        self.database.drop().await
    }
}

pub async fn run() -> TestApp {
    // Set the environment variable.
    unsafe { std::env::set_var("ENV_TEST", "1") };

    // Load configuration.
    let config = config::load();
    helpers::CONFIG.get_or_init(|| config.clone());

    // Connect to Redis.
    let redis = redis::open(&config).await;

    // Connect to PostgreSQL.
    let test_database = Database::open_test_database(config.clone().into())
        .await
        .expect("Failed to connect to the test database.");

    // Build the application state.
    let shared_state = Arc::new(AppState {
        config,
        db_pool: test_database.pool().clone(),
        redis: Mutex::new(redis),
    });

    // Run the api server.
    let server = api::server::build(shared_state);
    let server_handle = server.handle();
    tokio::spawn(server);

    wait_for_service(Duration::from_secs(5)).await;

    TestApp {
        server: server_handle,
        database: test_database,
    }
}

async fn wait_for_service(duration: Duration) {
    let timeout = Instant::now() + duration;
    loop {
        let url = helpers::build_path(API_V1, API_PATH_HEALTH);
        if let Ok(response) = reqwest::get(url.as_str()).await
            && response.status() == StatusCode::OK
        {
            break;
        }
        if Instant::now() > timeout {
            panic!("Could not start API Server in: {:?}", duration);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
