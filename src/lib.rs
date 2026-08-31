#![allow(clippy::all)]
#![warn(clippy::nursery)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::cognitive_complexity)]
// The actix-web request types are `!Send` by design, being reference counted per worker thread.
#![allow(clippy::future_not_send)]

pub mod api;
pub mod application;
pub mod domain;
pub mod infrastructure;
