use anyhow::Result;
use csv::ReaderBuilder;
use dotenv::dotenv;
use serde::Deserialize;
use sqlx::postgres::PgPool;
use std::{
    env,
    fs::File,
    io::{stdin, stdout, BufReader, Write},
};

/// Used to deserialize each row in the connections CSV
#[derive(Debug, Deserialize)]
struct ContactRow {
    #[serde(rename = "First Name")]
    first_name: String,

    #[serde(rename = "Last Name")]
    last_name: String,

    #[serde(rename = "URL")]
    url: String,

    #[serde(rename = "Email Address")]
    email_address: Option<String>,

    #[serde(rename = "Company")]
    company: String,

    #[serde(rename = "Position")]
    position: String,
}

/// Used to deserialize each row in the companies CSV
#[derive(Debug, Deserialize)]
struct CompanyRow {
    #[serde(rename = "Name")]
    name: String,

    #[serde(rename = "Website")]
    website: String,

    #[serde(rename = "Email")]
    email: Option<String>,

    #[serde(rename = "Industry")]
    industry: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");
    let pool = PgPool::connect(&database_url).await?;

    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "--add-company-id" {
        println!("Running migrations to add company_id column...");
        add_company_id();
    } else {
        println!("Usage:");
        println!("  cargo run -p companies-importer -- --import");
    }

    Ok(())
}

fn ask_yes_no(prompt: &str) -> bool {
    print!("{} ", prompt);
    stdout().flush().unwrap();

    let mut input = String::new();
    stdin().read_line(&mut input).expect("Failed to read input");
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

async fn import_connections(pool: &PgPool) -> Result<()> {
    let csv_path = env::var("CSV_PATH")
        .unwrap_or_else(|_| "./connections-importer/connections.csv".to_string());

    let file = File::open(csv_path)?;
    let buffered = BufReader::new(file);

    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(buffered);

    for result in rdr.deserialize() {
        let record: ContactRow = result?;

        sqlx::query(
            r#"
            INSERT INTO contacts
                (first_name, last_name, url, email_address, company, position)
            VALUES
                ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(&record.first_name)
        .bind(&record.last_name)
        .bind(&record.url)
        .bind(&record.email_address)
        .bind(&record.company)
        .bind(&record.position)
        .execute(pool)
        .await?;
    }

    println!("Connections data imported successfully!");
    Ok(())
}

async fn import_companies(pool: &PgPool) -> Result<()> {
    let companies_csv_path = env::var("COMPANIES_CSV_PATH")
        .unwrap_or_else(|_| "./connections-importer/companies.csv".to_string());

    let file = File::open(companies_csv_path)?;
    let buffered = BufReader::new(file);

    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(buffered);

    for result in rdr.deserialize() {
        let record: CompanyRow = result?;

        sqlx::query(
            r#"
            INSERT INTO companies
                (name, website, email, industry)
            VALUES
                ($1, $2, $3, $4)
            "#,
        )
        .bind(&record.name)
        .bind(&record.website)
        .bind(&record.email)
        .bind(&record.industry)
        .execute(pool)
        .await?;
    }

    println!("Companies data imported successfully!");
    Ok(())
}
