use clap::Parser;
use std::path::PathBuf;
use std::fs;
use anyhow::{Result, Context};

mod formatter;
use formatter::*;

#[derive(Parser)]
#[command(name = "shafu")]
#[command(about = "A Solidity formatter")]
#[command(version = "0.1.0")]
struct Cli {
    /// The Solidity file to format
    file: PathBuf,
    
    /// Write changes back to the file instead of printing to stdout
    #[arg(long)]
    write: bool,
}

/// Main entry point for formatting Solidity code
pub fn format_solidity(code: &str) -> Result<String> {
    // Run forge fmt first
    let formatted_code = run_forge_fmt(code)?;
    
    // Convert to lines for processing
    let mut lines: Vec<String> = formatted_code.lines().map(|s| s.to_string()).collect();
    
    // Apply formatting pipeline
    lines = convert_uint256_to_uint(lines);
    lines = format_import_statements(lines);
    lines = format_variable_declarations(lines);
    lines = format_function_declarations(lines);
    lines = format_constructors(lines);
    lines = format_require_statements(lines);
    lines = format_struct_assignments(lines);
    lines = format_variable_assignments(lines);
    lines = add_double_space_before_brace(lines);
    
    // Convert back to string and preserve trailing newlines
    let result = lines.join("\n");
    Ok(preserve_trailing_newline(code, &result))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    if !cli.file.exists() {
        anyhow::bail!("Error: {} not found", cli.file.display());
    }
    
    let content = fs::read_to_string(&cli.file)
        .with_context(|| format!("Failed to read file: {}", cli.file.display()))?;
    
    let formatted = format_solidity(&content)?;
    
    if cli.write {
        fs::write(&cli.file, &formatted)
            .with_context(|| format!("Failed to write file: {}", cli.file.display()))?;
        println!("Formatted {}", cli.file.display());
    } else {
        print!("{}", formatted);
    }
    
    Ok(())
}
