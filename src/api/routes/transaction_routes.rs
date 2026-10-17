use actix_web::web;

use crate::api::handlers::transaction_handlers::{get_transaction_handler, transfer_handler};
use crate::api::routes::get_or_head;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/transfer").route(web::post().to(transfer_handler)))
        .service(web::resource("/{id}").route(get_or_head().to(get_transaction_handler)));
}
