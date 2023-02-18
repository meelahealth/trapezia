pub mod appauth;
pub mod session;
pub mod strategy;
pub mod user;
pub mod username;

pub use user::postgres::PgPasswordResetBackend;

mod util;

#[cfg(feature = "deadpool")]
pub use util::deadpool::{PgHandle, PgPool};

#[cfg(feature = "deadpool")]
pub use user::postgres::DeadpoolPasswordResetBackend;
