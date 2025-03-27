use axum::{
    http::{
        header::{ACCEPT, AUTHORIZATION},
        Method,
    },
    routing::{get, post},
    Router,
};

use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
};

use shuttle_axum::ShuttleAxum;
use shuttle_openai::async_openai::{config::OpenAIConfig, Client};
use shuttle_runtime::{DeploymentMetadata, SecretStore};

use std::str::FromStr;

// --- Apalis + Cron-related imports
use apalis::cron::{CronStream, Schedule};
use apalis::layers::{DefaultRetryLayer, Extension, RetryLayer};
use apalis::postgres::PostgresStorage;
use apalis::prelude::timer::TokioTimer;
use apalis::prelude::{job_fn, Job, JobContext, Monitor, WorkerBuilder};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;

// --- Your modules
pub mod endpoints;
pub mod services;
pub mod state;
pub mod utils;

use crate::endpoints::speech::speech;
use crate::state::AppState;

// ---------------------
// 1) Define a simple cron Job
// ---------------------
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct Reminder(DateTime<Utc>);

impl Job for Reminder {
    const NAME: &'static str = "reminder::DailyReminder";
}

// We need `From<DateTime<Utc>>` for an Apalis cron job.
impl From<DateTime<Utc>> for Reminder {
    fn from(t: DateTime<Utc>) -> Self {
        Reminder(t)
    }
}

// This is the function our cron job calls regularly.
async fn say_hello_world(job: Reminder, ctx: JobContext) {
    // If we stored extra data using .layer(Extension(...)), we can retrieve it here
    if let Some(data) = ctx.data_opt::<CronjobData>() {
        println!(
            "[CRON] {} from `say_hello_world()`: the time is {:?}",
            data.message, job.0
        );
    } else {
        println!("[CRON] Hello world from `say_hello_world()`!");
    }
}

#[derive(Clone)]
struct CronjobData {
    message: String,
}

#[shuttle_runtime::main]
async fn main(
    #[shuttle_shared_db::Postgres] conn: String,
    #[shuttle_openai::OpenAI(api_key = "{secrets.OPENAI_API_KEY}")] openai: Client<OpenAIConfig>,
    #[shuttle_runtime::Metadata] metadata: DeploymentMetadata,
    #[shuttle_runtime::Secrets] secrets: SecretStore,
) -> ShuttleAxum {
    // ---------------------
    // 2) Set up your normal Axum + state
    // ---------------------
    let state = AppState::new(conn.clone(), openai)
        .await
        .map_err(|e| format!("Could not create application state: {e}"))?;

    state.seed().await;

    let openai_api_key = secrets
        .get("OPENAI_API_KEY")
        .ok_or("Missing OPENAI_API_KEY secret")?;
    std::env::set_var("OPENAI_API_KEY", &openai_api_key);

    let origin = if metadata.env == shuttle_runtime::Environment::Deployment {
        format!("{}.shuttle.app", metadata.project_name)
    } else {
        // local dev
        "127.0.0.1:8000".to_string()
    };

    let cors = CorsLayer::new()
        .allow_credentials(true)
        .allow_origin(vec![origin.parse().unwrap()])
        .allow_headers(vec![AUTHORIZATION, ACCEPT])
        .allow_methods(vec![Method::GET, Method::POST]);

    let router = Router::new()
        .route("/api/health", get(endpoints::health_check))
        .route("/api/auth/register", post(endpoints::auth::register))
        .route("/api/auth/login", post(endpoints::auth::login))
        .route(
            "/api/chat/conversations",
            get(endpoints::openai::get_conversation_list),
        )
        .route(
            "/api/chat/conversations/:id",
            get(endpoints::openai::fetch_conversation_messages)
                .post(endpoints::openai::send_message),
        )
        .route("/api/chat/create", post(endpoints::openai::create_chat))
        .route("/api/speech", post(speech))
        .layer(cors)
        .nest_service(
            "/",
            ServeDir::new("dist").not_found_service(ServeFile::new("dist/index.html")),
        )
        .with_state(state);

    // ---------------------
    // 3) Spin up Apalis + cron *in the background*
    // ---------------------

    // Create the Apalis Postgres storage using the same DB connection
    let storage = PostgresStorage::new_from_str(&conn)
        .await
        .expect("[CRON] Failed to create PostgresStorage");

    // Run Apalis migrations
    storage
        .setup()
        .await
        .expect("[CRON] Unable to run migrations");

    // Some data you'd like to pass into the cron job
    let cronjob_data = CronjobData {
        message: "Hello from cronjob!".to_string(),
    };

    // Build a "service" that Apalis uses to handle jobs
    let cron_service = ServiceBuilder::new()
        .layer(RetryLayer::new(DefaultRetryLayer)) // automatic retries
        .layer(Extension(cronjob_data)) // shared data for the job
        .service(job_fn(say_hello_world));

    // Decide on your cron schedule. Example: every 15s --> "*/15 * * * * *"
    // The default example from the blog is every second: "* * * * * *"
    let schedule =
        Schedule::from_str("*/15 * * * * *").expect("[CRON] Invalid schedule expression");

    // Build the worker that polls your CronStream
    let worker = WorkerBuilder::new("my-cron-worker")
        .with_storage(storage.clone())
        .stream(CronStream::new(schedule).timer(TokioTimer).to_stream())
        .build(cron_service);

    // Monitor that runs the worker in a separate task
    tokio::spawn(async move {
        Monitor::new()
            .register(worker)
            .run()
            .await
            .expect("[CRON] Worker monitor crashed");
    });

    // ---------------------
    // 4) Return your Axum router
    // ---------------------
    Ok(router.into())
}
