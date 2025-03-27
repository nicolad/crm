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
use serde_json;
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use std::str::FromStr;
use thiserror::Error;
use tracing::{debug, error, info, warn};

const JOKE_AGENT_PREAMBLE: &str = r#"
You are a humorous assistant that generates:
1. A creative, funny email subject about a random topic
2. A joke that matches the subject
Respond ONLY in valid JSON format with:
{ 
  "subject": "your funny subject here",
  "body": "your joke here (keep it work-appropriate)"
}"#;

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
            parameters: serde_json::json!({
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

        // Instantiate the Resend client from the environment variable
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

/// A little helper to strip out code fences (```json ... ```) from LLM responses,
/// in case the LLM includes them around valid JSON.
fn sanitize_json(input: &str) -> String {
    // Remove leading/trailing whitespace
    let trimmed = input.trim();

    // Replace any triple-backtick code fences
    let without_fences = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    without_fences.to_string()
}

async fn send_email_via_agent() -> AnyResult<()> {
    info!("Preparing to send email via agent...");

    // Create a new DeepSeek client from env
    let client = providers::deepseek::Client::from_env();
    debug!("DeepSeek client created.");

    let joke_agent = client
        .agent("deepseek-chat")
        .preamble(JOKE_AGENT_PREAMBLE)
        .max_tokens(300)
        .build();

    // Generate joke content
    let json_response = joke_agent
        .prompt("Create email content with a random joke")
        .await?;
    info!("Generated joke content: {}", json_response);

    // Sanitize the response in case it comes back wrapped in ```json fences
    let sanitized = sanitize_json(&json_response);

    // Parse JSON response
    let email_content: serde_json::Value = serde_json::from_str(&sanitized).map_err(|e| {
        error!("Failed to parse JSON response: {e}");
        EmailError(format!("Failed to parse JSON response: {e}"))
    })?;

    let subject = email_content["subject"]
        .as_str()
        .unwrap_or("Daily Laugh 😄");
    let body = email_content["body"]
        .as_str()
        .unwrap_or("Oops, the joke didn't load! But here's a smile anyway: 😊");

    // Create an agent dedicated to sending emails
    let email_agent = client
        .agent("deepseek-chat")
        .preamble("You are an email-sending agent. Use the send_email tool to send messages.")
        .tool(EmailSender)
        .max_tokens(1024)
        .build();
    debug!("Email agent built successfully.");

    // Construct email prompt with dynamic content
    let email_prompt = format!(
        r#"Send an email with:
        {{
            "to": ["nicolai.vadim@gmail.com"],
            "subject": "{}",
            "body": "<h2>Your Daily Dose of Humor</h2><p>{}</p><p>Have a great day! 🚀</p>"
        }}"#,
        subject, body
    );

    let response = email_agent.prompt(email_prompt).await;

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

        // You can provide a unique namespace name in `Config::new`
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
