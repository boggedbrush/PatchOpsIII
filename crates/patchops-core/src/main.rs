use std::{env, io::Read, process};

use serde::Deserialize;
use serde_json::Value;

use patchops_core::handle_command;

#[derive(Debug, Deserialize)]
struct Envelope {
    command: String,
    #[serde(default)]
    payload: Value,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let input = if stdin.trim().is_empty() {
        "{}"
    } else {
        stdin.trim()
    };

    let args: Vec<String> = env::args().collect();
    let (command, payload) = if let Some(command) = args.get(1) {
        (command.clone(), serde_json::from_str(input)?)
    } else {
        let envelope: Envelope = serde_json::from_str(input)?;
        (envelope.command, envelope.payload)
    };

    let output = handle_command(&command, payload)?;
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
