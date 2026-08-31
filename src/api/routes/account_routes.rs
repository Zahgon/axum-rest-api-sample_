use actix_web::web;

use crate::api::handlers::account_handlers::{
    add_account_handler, delete_account_handler, get_account_handler, list_accounts_handler,
    update_account_handler,
};
use crate::api::routes::get_or_head;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("")
            .route(get_or_head().to(list_accounts_handler))
            .route(web::post().to(add_account_handler)),
    )
    .service(
        web::resource("/{id}")
            .route(get_or_head().to(get_account_handler))
            .route(web::put().to(update_account_handler))
            .route(web::delete().to(delete_account_handler)),
    );
}
