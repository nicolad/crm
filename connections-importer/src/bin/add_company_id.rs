use anyhow::{Context, Result};
use dotenv::dotenv;
use sqlx::{postgres::PgPool, Postgres, Transaction};
use std::env;
use tracing::{debug, info, warn};
use tracing_subscriber::{fmt, EnvFilter};

// Add the indicatif crate
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Debug, sqlx::FromRow)]
struct Company {
    id: i32,
    name: String,
    website: String, // or Option<String> if you make the DB column nullable
}

#[derive(Debug, sqlx::FromRow)]
struct ExistingContact {
    id: i32,
    company: String,
}

async fn add_company_id(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;

    info!("Fetching contacts without company_id...");
    let contacts = sqlx::query_as::<_, ExistingContact>(
        "SELECT id, company FROM contacts WHERE company_id IS NULL",
    )
    .fetch_all(&mut *tx)
    .await
    .context("Failed to fetch contacts")?;

    info!("Found {} contacts to process.", contacts.len());

    // Initialize a progress bar
    let pb = ProgressBar::new(contacts.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar().template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({eta})"),
    );
    pb.set_message("Starting processing...");

    let mut updated_count = 0usize;
    let mut inserted_companies_count = 0usize;

    for (i, contact) in contacts.iter().enumerate() {
        debug!(
            "Processing contact index={} (ID={} with company='{}')",
            i, contact.id, contact.company
        );

        // Update the progress bar
        pb.set_message(format!("Processing contact ID={}", contact.id));
        pb.inc(1);

        if contact.company.trim().is_empty() {
            warn!(
                "Contact ID={} has an empty 'company' field; skipping.",
                contact.id
            );
            continue;
        }

        debug!(
            "Looking for existing company record for '{}'",
            contact.company
        );
        let maybe_company =
            sqlx::query_as::<_, Company>("SELECT id, name, website FROM companies WHERE name = $1")
                .bind(&contact.company)
                .fetch_optional(&mut *tx)
                .await
                .with_context(|| format!("Failed to search for company '{}'", contact.company))?;

        let company_id = match maybe_company {
            Some(company) => {
                debug!(
                    "Found existing company: id={}, name='{}', website='{}'",
                    company.id, company.name, company.website
                );
                company.id
            }
            None => {
                info!(
                    "No existing record for company '{}'; inserting a new company row.",
                    contact.company
                );
                let inserted = sqlx::query_as::<_, Company>(
                    r#"
                        INSERT INTO companies (name, website)
                        VALUES ($1, $2)
                        RETURNING id, name, website
                    "#,
                )
                .bind(&contact.company)
                .bind("https://placeholder.example.com") // or ""
                .fetch_one(&mut *tx)
                .await
                .with_context(|| format!("Failed to insert new company '{}'", contact.company))?;

                inserted_companies_count += 1;
                inserted.id
            }
        };

        debug!(
            "Updating contact ID={} to set company_id={}",
            contact.id, company_id
        );
        sqlx::query("UPDATE contacts SET company_id = $1 WHERE id = $2")
            .bind(company_id)
            .bind(contact.id)
            .execute(&mut *tx)
            .await
            .with_context(|| format!("Failed to update contact {}", contact.id))?;

        updated_count += 1;
    }

    info!(
        "Committing transaction. Updated {} contacts, inserted {} new companies.",
        updated_count, inserted_companies_count
    );
    pb.finish_with_message("Processing complete.");

    tx.commit().await?;
    println!(
        "Successfully updated {} contacts with company_id",
        updated_count
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env
    dotenv().ok();

    // Initialize logging
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    // Retrieve the database URL from .env
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set in .env")?;
    let pool = PgPool::connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    // If 'company_id' doesn't exist or is optional, you can add the column here:
    // sqlx::query!("ALTER TABLE contacts ADD COLUMN IF NOT EXISTS company_id INTEGER")
    //     .execute(&pool)
    //     .await
    //     .context("Failed to add company_id column to contacts")?;

    add_company_id(&pool)
        .await
        .context("Failed to get company ID for contacts")?;

    Ok(())
}
