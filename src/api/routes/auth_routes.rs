use actix_web::web;

use crate::api::handlers::auth_handlers::{
    cleanup_handler, login_handler, logout_handler, refresh_handler, revoke_all_handler,
    revoke_user_handler,
};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/login").route(web::post().to(login_handler)))
        .service(web::resource("/logout").route(web::post().to(logout_handler)))
        .service(web::resource("/refresh").route(web::post().to(refresh_handler)))
        .service(web::resource("/revoke-all").route(web::post().to(revoke_all_handler)))
        .service(web::resource("/revoke-user").route(web::post().to(revoke_user_handler)))
        .service(web::resource("/cleanup").route(web::post().to(cleanup_handler)));
}
