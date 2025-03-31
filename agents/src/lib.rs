// agents/src/lib.rs

use anyhow::Result;
use rig::{
    completion::{Prompt, ToolDefinition},
    providers,
    tool::Tool,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tracing_subscriber::fmt;

//
// ------------------ Data Structures & Error ------------------
//

#[derive(Deserialize)]
pub struct OperationArgs {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Error)]
#[error("Math error")]
pub struct MathError;

//
// ------------------ Tools ------------------
//

#[derive(Deserialize, Serialize)]
pub struct Adder;

impl Tool for Adder {
    const NAME: &'static str = "add";

    type Error = MathError;
    type Args = OperationArgs;
    type Output = i32;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "add".to_string(),
            description: "Add x and y together".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "x": {
                        "type": "number",
                        "description": "The first number to add"
                    },
                    "y": {
                        "type": "number",
                        "description": "The second number to add"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("[tool-call] Adding {} and {}", args.x, args.y);
        Ok(args.x + args.y)
    }
}

#[derive(Deserialize, Serialize)]
pub struct Subtract;

impl Tool for Subtract {
    const NAME: &'static str = "subtract";

    type Error = MathError;
    type Args = OperationArgs;
    type Output = i32;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        // This is just a demonstration of building a ToolDefinition from JSON
        serde_json::from_value(json!({
            "name": "subtract",
            "description": "Subtract y from x (x - y)",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": {
                        "type": "number",
                        "description": "The number to subtract from"
                    },
                    "y": {
                        "type": "number",
                        "description": "The number to subtract"
                    }
                }
            }
        }))
        .expect("Tool Definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        println!("[tool-call] Subtracting {} from {}", args.y, args.x);
        Ok(args.x - args.y)
    }
}

//
// ------------------ Example Demonstration Function ------------------
//

/// Example function that demonstrates how to use the crate's tools.
/// You could remove this if you only want the library code.
#[tokio::main]
pub async fn run_example() -> Result<()> {
    // Initialize logging
    fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    let client = providers::deepseek::Client::from_env();

    // 1) Basic agent that just answers a question
    let agent = client
        .agent("deepseek-chat")
        .preamble("You are a helpful assistant.")
        .build();

    let answer = agent.prompt("Tell me a joke").await?;
    println!("Answer: {answer}");

    // 2) An agent that uses the calculator tools
    let calculator_agent = client
        .agent(providers::deepseek::DEEPSEEK_CHAT)
        .preamble("You are a calculator here to help the user perform arithmetic operations. Use the tools provided to answer the user's question.")
        .max_tokens(1024)
        .tool(Adder)
        .tool(Subtract)
        .build();

    println!("Calculate 2 - 5:");
    let calc_result = calculator_agent.prompt("Calculate 2 - 5").await?;
    println!("Calculator Agent says: {}", calc_result);

    Ok(())
}
