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

#[derive(Clone)]
struct CronjobData {
    message: String,
}

impl CronjobData {
    fn execute(&self, _item: Reminder) {
        println!("{} from CronjobData::execute()!", &self.message);
    }
}

/// A custom error for the email-sending tool.
/// Deriving `thiserror::Error` implements `std::error::Error` so it
/// satisfies the rig-core `Tool::Error` requirement.
#[derive(Error, Debug)]
#[error("Email error: {0}")]
struct EmailError(String);

/// The arguments our "send_email" tool accepts.
#[derive(Deserialize, Serialize)]
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
            // This must be a `String`, not `&'static str`
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
        // Instantiate the Resend client from `RESEND_API_KEY`
        // Must be set in your environment or .env file
        let resend = Resend::default();

        // This `from` must be a verified sender/domain in Resend:
        let from = "Acme <onboarding@resend.dev>";
        let email_options =
            CreateEmailBaseOptions::new(from, &args.to, &args.subject).with_html(&args.body);

        // Attempt to send the email
        match resend.emails.send(email_options).await {
            Ok(_) => Ok("Email sent successfully!".to_string()),
            Err(e) => Err(EmailError(format!("Failed to send email: {e}"))),
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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // Create a new DeepSeek client from env
    let client = providers::deepseek::Client::from_env();

    // Create an agent that is dedicated to sending emails
    // and attach the "EmailSender" tool
    let email_agent = client
        .agent("deepseek-chat")
        .preamble("You are an email-sending agent. Use the send_email tool to send messages.")
        .tool(EmailSender)
        .max_tokens(1024)
        .build();

    // Prompt your agent with an instruction to send an email
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
        .await?;

    println!("Agent response: {response}");

    Ok(())
}

async fn say_hello_world(job: Reminder, svc: Data<CronjobData>) {
    println!("Hello world from send_reminder()!");
    // this executes CronjobData::execute()
    if let Err(e) = send_email_via_agent().await {
        eprintln!("Error sending email: {e}");
    }
    svc.execute(job);
}

#[shuttle_runtime::main]
async fn shuttle_main(
    #[shuttle_shared_db::Postgres] conn_string: String,
) -> Result<MyService, shuttle_runtime::Error> {
    dotenv().ok();

    let db = PgPoolOptions::new()
        .min_connections(5)
        .max_connections(5)
        .connect(&conn_string)
        .await
        .unwrap();

    Ok(MyService { db })
}

// Customize this struct with things from `shuttle_main` needed in `bind`,
// such as secrets or database connections
struct MyService {
    db: PgPool,
}

#[shuttle_runtime::async_trait]
impl shuttle_runtime::Service for MyService {
    async fn bind(self, _addr: std::net::SocketAddr) -> Result<(), shuttle_runtime::Error> {
        // set up storage
        PostgresStorage::setup(&self.db)
            .await
            .expect("Unable to run migrations :(");

        let config = Config::new("reminder::DailyReminder");
        let storage = PostgresStorage::new_with_config(self.db, config);

        // Create a schedule that runs every 2 minutes (at second 0).
        // 0 */2 * * * * means:
        // second: 0
        // minute: every 2
        // hour/day-of-month/month/day-of-week: any
        let schedule = Schedule::from_str("0 */2 * * * *").expect("Couldn't create the schedule!");

        let cron_service_ext = CronjobData {
            message: "Hello world".to_string(),
        };

        let persisted_cron = CronStream::new(schedule).pipe_to_storage(storage);

        // create a worker that uses the service created from the cronjob
        let worker = WorkerBuilder::new("morning-cereal")
            .data(cron_service_ext)
            .retry(RetryPolicy::retries(5))
            .backend(persisted_cron)
            .build_fn(say_hello_world);

        // start your worker up
        worker.run().await;

        Ok(())
    }
}
