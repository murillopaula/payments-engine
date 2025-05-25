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
/// FIX: Manually writes records for formatting control and sorts by client_id.
pub fn write_accounts<W: Write>(
    engine: &PaymentEngine,
    writer: W,
) -> Result<(), PaymentError> {
    let mut wtr = csv::Writer::from_writer(writer);
    let mut accounts = engine.get_accounts(); // This returns Vec<OutputRecord>

    // Sort by client ID for deterministic output (good for testing)
    accounts.sort_by_key(|a| a.client_id);

    wtr.write_record(&["client", "available", "held", "total", "locked"])?;

    for account_record in accounts {
        wtr.write_record(&[
            account_record.client_id.to_string(),
            format!("{:.4}", account_record.available), // Format to 4dp
            format!("{:.4}", account_record.held),     // Format to 4dp
            format!("{:.4}", account_record.total),   // Format to 4dp
            account_record.locked.to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PaymentEngine; // Make sure engine is in scope
    use crate::errors::PaymentError;
    use crate::models::{InputRecord}; // Make sure InputRecord is in scope
    use csv; // Make sure csv is in scope
    use std::io::Cursor;

    /// Helper to run tests with CSV input and capture output.
    /// FIX: Trims the final output string.
    fn run_test_csv(input_csv: &str) -> Result<String, PaymentError> {
        let mut engine = PaymentEngine::new();

        let mut rdr = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(input_csv.as_bytes());

        for result in rdr.deserialize() {
            let record: InputRecord = result?;
            engine.process(record)?;
        }

        let mut output_buf = Vec::new();
        write_accounts(&engine, Cursor::new(&mut output_buf))?;

        // Trim to remove potential trailing newline for easier comparison
        Ok(String::from_utf8(output_buf).unwrap().trim().to_string())
    }

    /// FIX: Updated expected string with 4dp and sorted. Using direct assert_eq.
    #[test]
    fn test_simple_csv_processing() {
        let input = "type,client,tx,amount\n\
                     deposit,1,1,1.0\n\
                     deposit,2,2,2.0\n\
                     deposit,1,3,2.0\n\
                     withdrawal,1,4,1.5\n\
                     withdrawal,2,5,3.0"; // This should fail silently

        let expected = "client,available,held,total,locked\n\
                        1,1.5000,0.0000,1.5000,false\n\
                        2,2.0000,0.0000,2.0000,false";

        let result = run_test_csv(input).unwrap();
        assert_eq!(result, expected);
    }

    /// FIX: Updated expected string with 4dp. Using direct assert_eq.
    #[test]
    fn test_dispute_csv_processing() {
        let input = "type,client,tx,amount\n\
                     deposit,1,1,100.0\n\
                     deposit,1,2,50.0\n\
                     dispute,1,1,\n\
                     withdrawal,1,3,20.0\n\
                     resolve,1,1,\n\
                     dispute,1,2,\n\
                     chargeback,1,2,";

        let expected = "client,available,held,total,locked\n\
                        1,80.0000,0.0000,80.0000,true";

        let result = run_test_csv(input).unwrap();
        assert_eq!(result, expected);
    }

    /// FIX: Updated expected string with 4dp and sorted. Using direct assert_eq.
    #[test]
    fn test_ignore_errors_csv() {
        let input = "type,client,tx,amount\n\
                     deposit,1,1,100.0\n\
                     dispute,1,99,\n\
                     resolve,1,1,\n\
                     withdrawal,1,2,200.0\n\
                     deposit,2,3,50.0\n\
                     chargeback,1,1,"; // This should be ignored as tx 1 is not disputed

        let expected = "client,available,held,total,locked\n\
                        1,100.0000,0.0000,100.0000,false\n\
                        2,50.0000,0.0000,50.0000,false";

        let result = run_test_csv(input).unwrap();
        assert_eq!(result, expected);
    }

    /// FIX: Updated expected string with 4dp. Using direct assert_eq.
    #[test]
    fn test_whitespace_and_precision() {
        let input = "type, client,  tx, amount\n\
                     deposit,  1,   1, 1.1234\n\
                     deposit,  1,   2,  2.5  \n\
                     withdrawal, 1, 3, 0.5";
        let expected = "client,available,held,total,locked\n\
                        1,3.1234,0.0000,3.1234,false";

        let result = run_test_csv(input).unwrap();
        assert_eq!(result, expected);
    }
}
