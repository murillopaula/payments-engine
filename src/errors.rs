// src/errors.rs
use thiserror::Error;

/// Custom error types for the payment engine.
#[derive(Error, Debug)]
pub enum PaymentError {
    #[error("CSV processing error: {0}")]
    Csv(#[from] csv::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Decimal parsing error: {0}")]
    Decimal(#[from] rust_decimal::Error),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
}
