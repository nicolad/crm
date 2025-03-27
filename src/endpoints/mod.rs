pub mod auth;
pub mod openai;
pub mod speech;

pub async fn health_check() -> &'static str {
    "Hello, world!"
}
