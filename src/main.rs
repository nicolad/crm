use anyhow::Result as AnyResult;
use apalis::layers::retry::RetryPolicy;
use apalis::prelude::*;
use apalis_cron::CronStream;
use apalis_cron::Schedule;
use apalis_sql::postgres::PostgresStorage;
use apalis_sql::Config;
use chrono::{DateTime, Utc};
use dotenv::dotenv;
use resend_rs::types::CreateEmailBaseOptions;
use resend_rs::Resend;
use rig::{
    completion::{Prompt, ToolDefinition},
    providers,
    tool::Tool,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use std::str::FromStr;
use thiserror::Error;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
struct CronjobData {
    message: String,
}

impl CronjobData {
    fn execute(&self, _item: Reminder) {
        println!("{} from CronjobData::execute()!", &self.message);
        info!("CronjobData::execute() finished for item: {:?}", _item);
    }
}

/// A custom error for the email-sending tool.
#[derive(Error, Debug)]
#[error("Email error: {0}")]
struct EmailError(String);

/// The arguments our "send_email" tool accepts.
#[derive(Deserialize, Serialize, Debug)]
struct EmailArgs {
    /// Recipient emails
    to: Vec<String>,
    /// Subject of the email
    subject: String,
    /// Body (HTML or plain text)
    body: String,
}

/// A tool that sends an email using the Resend API.
#[derive(Deserialize, Serialize)]
struct EmailSender;

impl Tool for EmailSender {
    const NAME: &'static str = "send_email";

    type Error = EmailError;
    type Args = EmailArgs;
    type Output = String;

    /// The JSON schema / definition for this tool.
    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "send_email".to_string(),
            description: "Send an email using the Resend API.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of recipient email addresses"
                    },
                    "subject": {
                        "type": "string",
                        "description": "The subject line for the email"
                    },
                    "body": {
                        "type": "string",
                        "description": "The body of the email, in HTML or plain text"
                    }
                },
                "required": ["to", "subject", "body"]
            }),
        }
    }

    /// The actual implementation that calls Resend to send the email.
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Log the args so we can confirm what we're sending
        debug!("EmailSender::call() invoked with args: {:?}", args);

        // Check environment variable for Resend
        match env::var("RESEND_API_KEY") {
            Ok(key) => {
                debug!("RESEND_API_KEY is present, length: {}", key.len());
            }
            Err(_) => {
                warn!("RESEND_API_KEY is not set. Make sure it's defined in .env or environment variables.");
            }
        }

        // Instantiate the Resend client from `RESEND_API_KEY`
        let resend = Resend::default();
        // This `from` must be a verified sender/domain in Resend:
        let from = "Acme <onboarding@resend.dev>";
        let email_options =
            CreateEmailBaseOptions::new(from, &args.to, &args.subject).with_html(&args.body);

        // Attempt to send the email
        info!("Sending request to Resend...");
        match resend.emails.send(email_options).await {
            Ok(_) => {
                info!("Email sent successfully!");
                Ok("Email sent successfully!".to_string())
            }
            Err(e) => {
                error!("Failed to send email via Resend: {e}");
                Err(EmailError(format!("Failed to send email: {e}")))
            }
        }
    }
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct Reminder(DateTime<Utc>);

impl From<DateTime<Utc>> for Reminder {
    fn from(t: DateTime<Utc>) -> Self {
        Reminder(t)
    }
}

async fn send_email_via_agent() -> AnyResult<()> {
    // Setup basic logging or tracing
    // Note: If this is called multiple times, you might see a warning about `tracing_subscriber` being initialized multiple times.
    // For debugging, it's usually fine; otherwise consider centralizing this in main or shuttle_main.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    info!("Preparing to send email via agent...");

    // Create a new DeepSeek client from env
    let client = providers::deepseek::Client::from_env();
    debug!("DeepSeek client created.");

    // Create an agent dedicated to sending emails
    let email_agent = client
        .agent("deepseek-chat")
        .preamble("You are an email-sending agent. Use the send_email tool to send messages.")
        .tool(EmailSender)
        .max_tokens(1024)
        .build();
    debug!("Email agent built successfully.");

    // Prompt the agent with instruction to send an email
    let response = email_agent
        .prompt(
            r#"
            Send an email with the following JSON:
            {
              "to": ["nicolai.vadim@gmail.com"],
              "subject": "Greetings from Rust Agent (via Cron Job)",
              "body": "<p>Hello from our Apalis Cron job!</p>"
            }
            "#,
        )
        .await;

    match response {
        Ok(r) => {
            info!("Agent response: {r}");
            Ok(())
        }
        Err(e) => {
            error!("Failed to get a response from the email agent: {e}");
            Err(e.into())
        }
    }
}

async fn say_hello_world(job: Reminder, svc: Data<CronjobData>) {
    info!("say_hello_world() job invoked for Reminder: {:?}", job);
    println!("Hello world from send_reminder()!");

    // Attempt to send email
    if let Err(e) = send_email_via_agent().await {
        error!("Error sending email: {e}");
        eprintln!("Error sending email: {e}");
    }

    svc.execute(job);
}

#[shuttle_runtime::main]
async fn shuttle_main(
    #[shuttle_shared_db::Postgres] conn_string: String,
) -> Result<MyService, shuttle_runtime::Error> {
    dotenv().ok();
    info!("shuttle_main: Starting up with provided Postgres connection string.");

    // Create connection pool
    let db = PgPoolOptions::new()
        .min_connections(5)
        .max_connections(5)
        .connect(&conn_string)
        .await
        .expect("Failed to connect to the Postgres database");

    info!("Database connection pool established successfully.");

    Ok(MyService { db })
}

// Customize this struct with things from `shuttle_main` needed in `bind`.
struct MyService {
    db: PgPool,
}

#[shuttle_runtime::async_trait]
impl shuttle_runtime::Service for MyService {
    async fn bind(self, _addr: std::net::SocketAddr) -> Result<(), shuttle_runtime::Error> {
        info!("MyService::bind() called. Setting up storage and cron worker...");

        // set up storage
        PostgresStorage::setup(&self.db)
            .await
            .expect("Unable to run migrations :(");
        info!("PostgresStorage migrations completed successfully.");

        let config = Config::new("reminder::DailyReminder");
        let storage = PostgresStorage::new_with_config(self.db.clone(), config);
        debug!("PostgresStorage with custom config created.");

        // Create a schedule that runs every 2 minutes (at second 0).
        let schedule_str = "0 */2 * * * *";
        info!("Using schedule: {}", schedule_str);
        let schedule = Schedule::from_str(schedule_str)
            .expect("Couldn't create the schedule from cron expression!");

        let cron_service_ext = CronjobData {
            message: "Hello world".to_string(),
        };

        let persisted_cron = CronStream::new(schedule).pipe_to_storage(storage);
        debug!("Cron stream setup complete; now building worker.");

        // Build worker
        let worker = WorkerBuilder::new("morning-cereal")
            .data(cron_service_ext)
            .retry(RetryPolicy::retries(5))
            .backend(persisted_cron)
            .build_fn(say_hello_world);

        info!("Worker built; running worker now.");
        worker.run().await;

        Ok(())
    }
}
