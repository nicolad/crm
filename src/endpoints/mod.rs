pub mod auth;

pub async fn health_check() -> &'static str {
    "Hello, world!"
}
