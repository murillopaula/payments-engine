use crate::engine::PaymentEngine;
use crate::errors::PaymentError;
use crate::models::InputRecord;
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Processes transactions from a CSV file.
pub fn process_transactions<P: AsRef<Path>>(
    file_path: P,
    engine: &mut PaymentEngine,
) -> Result<(), PaymentError> {
    let file = File::open(file_path)?;
    let mut rdr = csv::ReaderBuilder::new()
        .trim(csv::Trim::All) // Handle potential whitespaces
        .flexible(true) // Allow traiiling commas
        .from_reader(file);

    for result in rdr.deserialize() {
        let record: InputRecord = match result {
            Ok(rec) => rec,
            Err(e) => {
                eprintln!("Warning: Skipping bad record: {}", e);
                continue;
            }
        };

        if let Err(e) = engine.process(record) {
            eprintln!("Warning: Error processing transaction: {}", e);
        }
    }
    Ok(())
}

/// Writes account states to a CSV format.
pub fn write_accounts<W: Write>(engine: &PaymentEngine, writer: W) -> Result<(), PaymentError> {
    let mut wtr = csv::Writer::from_writer(writer);
    let mut accounts = engine.get_accounts();

    // Sort by client ID for deterministic output (good for testing)
    accounts.sort_by_key(|a| a.client_id);

    wtr.write_record(["client", "available", "held", "total", "locked"])?;

    for account_record in accounts {
        wtr.write_record(&[
            account_record.client_id.to_string(),
            format!("{:.4}", account_record.available),
            format!("{:.4}", account_record.held),
            format!("{:.4}", account_record.total),
            account_record.locked.to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PaymentEngine;
    use crate::errors::PaymentError;
    use crate::models::InputRecord;
    use rstest::rstest;
    use std::io::Cursor;

    /// Helper to run tests with CSV input and capture output.
    fn run_test_csv(input_csv: &str) -> Result<String, PaymentError> {
        let mut engine = PaymentEngine::new();

        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(input_csv.as_bytes());

        for result in rdr.deserialize() {
            let record: InputRecord = result?;
            // Ignore errors, just like in production
            let _ = engine.process(record);
        }

        let mut output_buf = Vec::new();
        write_accounts(&engine, Cursor::new(&mut output_buf))?;

        Ok(String::from_utf8(output_buf).unwrap().trim().to_string())
    }

    #[rstest]
    #[case(
        // Simple deposits and withdrawals
        "type,client,tx,amount\n\
        deposit,1,1,1.0\n\
        deposit,2,2,2.0\n\
        deposit,1,3,2.0\n\
        withdrawal,1,4,1.5\n\
        withdrawal,2,5,3.0",
        "client,available,held,total,locked\n\
        1,1.5000,0.0000,1.5000,false\n\
        2,2.0000,0.0000,2.0000,false"
    )]
    #[case(
        // Dispute/resolve/chargeback scenario
        "type,client,tx,amount\n\
        deposit,1,1,100.0\n\
        deposit,1,2,50.0\n\
        dispute,1,1,\n\
        withdrawal,1,3,20.0\n\
        resolve,1,1,\n\
        dispute,1,2,\n\
        chargeback,1,2,",
        "client,available,held,total,locked\n\
        1,80.0000,0.0000,80.0000,true"
    )]
    #[case(
        // Ignore errors and invalid operations
        "type,client,tx,amount\n\
        deposit,1,1,100.0\n\
        dispute,1,99,\n\
        resolve,1,1,\n\
        withdrawal,1,2,200.0\n\
        deposit,2,3,50.0\n\
        chargeback,1,1,",
        "client,available,held,total,locked\n\
        1,100.0000,0.0000,100.0000,false\n\
        2,50.0000,0.0000,50.0000,false"
    )]
    #[case(
        // Whitespace and precision handling
        "type, client,  tx, amount\n\
        deposit,  1,   1, 1.1234\n\
        deposit,  1,   2,  2.5  \n\
        withdrawal, 1, 3, 0.5",
        "client,available,held,total,locked\n\
        1,3.1234,0.0000,3.1234,false"
    )]
    #[case(
        // Invalid withdrawal triggers error branch
        "type,client,tx,amount\n\
        withdrawal,1,1,0.0",
        "client,available,held,total,locked"
    )]
    fn test_csv_processing_cases(#[case] input: &str, #[case] expected: &str) {
        let result = super::tests::run_test_csv(input).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_process_transactions_error_branch_is_covered() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create a temporary CSV file with an invalid withdrawal (amount 0)
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "type,client,tx,amount\nwithdrawal,1,1,0.0").unwrap();

        let mut engine = PaymentEngine::new();
        // This will trigger the error branch and cover the eprintln! line
        let result = process_transactions(temp_file.path(), &mut engine);

        assert!(result.is_ok());
        // Optionally, check that no accounts were created
        assert!(engine.get_accounts().is_empty());
    }
}
