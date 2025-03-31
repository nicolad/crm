use anyhow::{Context, Result};
use dotenv::dotenv;
use rig::{
    pipeline::{self, agent_ops, TryOp},
    providers::deepseek,
    try_parallel,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::env;
use tracing::{debug, info};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// A record containing extracted names
pub struct Names {
    /// The names extracted from the text
    pub names: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// A record containing extracted topics
pub struct Topics {
    /// The topics extracted from the text
    pub topics: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema, Serialize)]
/// A record containing extracted sentiment
pub struct Sentiment {
    /// The sentiment of the text (-1 being negative, 1 being positive)
    pub sentiment: f64,
    /// The confidence of the sentiment
    pub confidence: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct Company {
    id: i32,
    name: String,
    website: String,
}

/// Fetches only the first 10 companies from the `companies` table.
async fn get_first_10_companies(pool: &PgPool) -> Result<Vec<Company>> {
    info!("Attempting to fetch the first 10 companies from the database...");
    debug!("Preparing to execute SQL query for fetching companies...");
    let companies = sqlx::query_as::<_, Company>(
        r#"
        SELECT id, name, website
        FROM companies
        ORDER BY id
        LIMIT 10
        "#,
    )
    .fetch_all(pool)
    .await
    .context("Failed to fetch the first 10 companies")?;

    info!("Successfully fetched {} companies.", companies.len());
    debug!("Companies fetched: {:?}", companies);
    debug!("Returning fetched companies from get_first_10_companies function...");
    Ok(companies)
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1) Load environment variables and initialize logging
    debug!("Starting program execution...");
    info!("Loading environment variables from .env...");
    dotenv().ok();
    info!("Environment variables loaded.");

    info!("Initializing tracing/logging...");
    fmt().with_env_filter(EnvFilter::from_default_env()).init();
    info!("Tracing/logging initialized.");

    // 2) Connect to the database
    info!("Retrieving DATABASE_URL from environment...");
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set in .env")?;
    debug!("DATABASE_URL successfully retrieved (value not printed for security).");

    info!("Connecting to the database...");
    let pool = PgPool::connect(&database_url)
        .await
        .context("Failed to connect to database")?;
    info!("Successfully connected to the database.");

    // 3) Fetch the first 10 companies
    info!("Fetching the first 10 companies from the 'companies' table...");
    let first_ten_companies = get_first_10_companies(&pool).await?;
    info!(
        "Fetched {} companies from the database.",
        first_ten_companies.len()
    );

    // 4) Set up DeepSeek client and extractors
    info!("Initializing DeepSeek client...");
    let client = deepseek::Client::from_env();
    info!("DeepSeek client initialized.");

    info!("Building names extractor...");
    let names_extractor = client
        .extractor::<Names>(deepseek::DEEPSEEK_CHAT)
        .preamble("Extract names (e.g.: of people or places) from the given text.")
        .build();
    info!("Names extractor built.");

    info!("Building topics extractor...");
    let topics_extractor = client
        .extractor::<Topics>(deepseek::DEEPSEEK_CHAT)
        .preamble("Extract topics from the given text.")
        .build();
    info!("Topics extractor built.");

    info!("Building sentiment extractor...");
    let sentiment_extractor = client
        .extractor::<Sentiment>(deepseek::DEEPSEEK_CHAT)
        .preamble(
            "Extract sentiment (and how confident you are of the sentiment) from the given text.",
        )
        .build();
    info!("Sentiment extractor built.");

    // 5) Create a pipeline chain to extract names, topics, and sentiment
    info!("Setting up pipeline chain...");
    let chain = pipeline::new()
        .chain(try_parallel!(
            agent_ops::extract(names_extractor),
            agent_ops::extract(topics_extractor),
            agent_ops::extract(sentiment_extractor),
        ))
        .map_ok(|(names, topics, sentiment)| {
            debug!("Pipeline chain outputs received. Constructing final analysis string...");
            format!(
                "Extracted names: {}\nExtracted topics: {}\nExtracted sentiment: {} (confidence: {})",
                names.names.join(", "),
                topics.topics.join(", "),
                sentiment.sentiment,
                sentiment.confidence
            )
        });
    info!("Pipeline chain set up successfully.");

    // 6) Prepare text for each of the first 10 companies
    info!("Preparing text for DeepSeek analysis...");
    let company_texts: Vec<String> = first_ten_companies
        .iter()
        .map(|c| {
            let text = format!("Company: {}, Website: {}", c.name, c.website);
            debug!("Prepared text for company ID {}: {}", c.id, text);
            text
        })
        .collect();
    info!(
        "Prepared text for {} companies to analyze.",
        company_texts.len()
    );

    // 7) Run the pipeline on the company data
    info!("Starting parallel batch call to pipeline with concurrency=4...");
    let responses = chain.try_batch_call(4, company_texts).await?;
    info!("Pipeline batch call completed. Processing responses...");

    // 8) Print the results
    for (company, analysis) in first_ten_companies.iter().zip(responses.iter()) {
        debug!(
            "Analysis result for company ID {}: {}",
            company.id, analysis
        );
        println!("=== Company Analysis (ID: {}) ===", company.id);
        println!("Name: {}", company.name);
        println!("Website: {}", company.website);
        println!("Analysis:\n{analysis}\n");
    }

    info!("All analyses completed successfully. Exiting program.");
    Ok(())
}
