use std::env;
use std::io;
use std::process;

mod csv_handler;
mod engine;
mod errors;
mod models;

fn main() {
    // 1. Get the input file path from command-line arguments.
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input_csv_file>", args[0]);
        process::exit(1);
    }
    let input_path = &args[1];

    // 2. Process the transactions.
    let mut engine = engine::PaymentEngine::new();
    if let Err(e) = csv_handler::process_transactions(input_path, &mut engine) {
        eprintln!("Error processing transactions: {}", e);
        process::exit(1);
    }

    // 3. Write the final account states to stdout.
    if let Err(e) = csv_handler::write_accounts(&engine, io::stdout()) {
        eprintln!("Error writing accounts: {}", e);
        process::exit(1);
    }
}
