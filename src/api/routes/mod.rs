use actix_web::{
    Route,
    guard::{Any, Get, Head},
    web,
};

pub mod account_routes;
pub mod auth_routes;
pub mod transaction_routes;
pub mod user_routes;

/// Builds a route accepting both `GET` and `HEAD` requests.
pub fn get_or_head() -> Route {
    web::route().guard(Any(Get()).or(Head()))
}
