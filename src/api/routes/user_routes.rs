use actix_web::web;

use crate::api::handlers::user_handlers::{
    add_user_handler, delete_user_handler, get_user_handler, list_users_handler,
    update_user_handler,
};
use crate::api::routes::get_or_head;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("")
            .route(get_or_head().to(list_users_handler))
            .route(web::post().to(add_user_handler)),
    )
    .service(
        web::resource("/{id}")
            .route(get_or_head().to(get_user_handler))
            .route(web::put().to(update_user_handler))
            .route(web::delete().to(delete_user_handler)),
    );
}
