-- Create contacts table
CREATE TABLE IF NOT EXISTS contacts (
    id SERIAL PRIMARY KEY,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    url TEXT NOT NULL,
    email_address TEXT,
    company TEXT NOT NULL,
    position TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

-- Create companies table
CREATE TABLE IF NOT EXISTS companies (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    website TEXT NOT NULL,
    email TEXT,
    industry TEXT,
    created_at TIMESTAMP DEFAULT NOW()
);