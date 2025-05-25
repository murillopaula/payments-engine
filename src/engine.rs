use crate::errors::PaymentError;
use crate::models::{Account, InputRecord, TransactionInfo, TransactionState, TransactionType};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct PaymentEngine {
    accounts: HashMap<u16, Account>,
    transactions: HashMap<u32, TransactionInfo>,
}

impl PaymentEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Retrieves an account, creating it if it doesn't exist.
    fn get_or_create_account(&mut self, client_id: u16) -> &mut Account {
        self.accounts
            .entry(client_id)
            .or_insert_with(|| Account::new(client_id))
    }

    /// Processes a single transaction record.
    pub fn process(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let tx_id = record.tx_id;

        // Check if the transaction ID is already processed (except for dispute/resolve/chargeback)
        if matches!(
            record.record_type,
            TransactionType::Deposit | TransactionType::Withdrawal
        ) && self.transactions.contains_key(&tx_id)
        {
            // Ignore duplicate deposit/withdrawal transactions silently or log a warning.
            // For this exercise, we'll ignore them.
            return Ok(());
        }

        match record.record_type {
            TransactionType::Deposit => self.handle_deposit(record),
            TransactionType::Withdrawal => self.handle_withdrawal(record),
            TransactionType::Dispute => self.handle_dispute(record),
            TransactionType::Resolve => self.handle_resolve(record),
            TransactionType::Chargeback => self.handle_chargeback(record),
        }
    }

    fn handle_deposit(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let amount = record.amount.ok_or_else(|| {
            PaymentError::InvalidTransaction(format!("Deposit {} missing amount", record.tx_id))
        })?;
        if amount <= Decimal::ZERO {
            return Err(PaymentError::InvalidTransaction(format!(
                "Deposit amount for tx {} must be positive",
                record.tx_id
            )));
        }

        let account = self.get_or_create_account(record.client_id);
        // No locked check needed here, account.deposit will handle it (or allow it).
        account.deposit(amount);

        // Store deposit info for potential disputes.
        self.transactions.insert(
            record.tx_id,
            TransactionInfo {
                client_id: record.client_id,
                amount,
                state: TransactionState::Normal,
            },
        );
        Ok(())
    }

    fn handle_withdrawal(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let amount = record.amount.ok_or_else(|| {
            PaymentError::InvalidTransaction(format!("Withdrawal {} missing amount", record.tx_id))
        })?;
        if amount <= Decimal::ZERO {
            return Err(PaymentError::InvalidTransaction(format!(
                "Withdrawal amount for tx {} must be positive",
                record.tx_id
            )));
        }

        let account = self.get_or_create_account(record.client_id);
        // account.withdraw will check for locked status.
        account.withdraw(amount); // We ignore the bool result as per spec.
        Ok(())
    }

    fn handle_dispute(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let tx_id = record.tx_id;
        let tx_info_opt = self.transactions.get(&tx_id).copied(); // Use copied to avoid mutable borrow issues

        let tx_info = match tx_info_opt {
            Some(info) => info,
            None => return Ok(()), // Ignore if tx doesn't exist.
        };

        if tx_info.state != TransactionState::Normal {
            return Ok(()); // Ignore if not normal.
        }

        let account = match self.accounts.get_mut(&tx_info.client_id) {
            Some(acc) => acc,
            None => return Ok(()),
        };

        if account.hold(tx_info.amount) {
            if let Some(tx_to_update) = self.transactions.get_mut(&tx_id) {
                tx_to_update.state = TransactionState::Disputed;
            }
        }
        Ok(())
    }

    fn handle_resolve(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let tx_id = record.tx_id;
        let tx_info_opt = self.transactions.get(&tx_id).copied();

        let tx_info = match tx_info_opt {
            Some(info) => info,
            None => return Ok(()),
        };

        if tx_info.state != TransactionState::Disputed {
            return Ok(());
        }

        let account = match self.accounts.get_mut(&tx_info.client_id) {
            Some(acc) => acc,
            None => return Ok(()),
        };

        if account.release(tx_info.amount) {
            self.transactions.remove(&tx_id);
        }

        Ok(())
    }

    fn handle_chargeback(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let tx_id = record.tx_id;
        let tx_info_opt = self.transactions.get(&tx_id).copied();

        let tx_info = match tx_info_opt {
            Some(info) => info,
            None => return Ok(()),
        };

        if tx_info.state != TransactionState::Disputed {
            return Ok(());
        }

        let account = match self.accounts.get_mut(&tx_info.client_id) {
            Some(acc) => acc,
            None => return Ok(()),
        };

        if account.chargeback(tx_info.amount) {
            self.transactions.remove(&tx_id);
        }
        Ok(())
    }

    /// Returns a vector of all accounts formatted for output.
    pub fn get_accounts(&self) -> Vec<crate::models::OutputRecord> {
        self.accounts
            .values()
            .map(|acc| acc.to_output_record())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, TransactionType};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    #[rstest]
    fn test_locked_account_behavior() {
        let mut acc = Account::new(1);
        acc.available = dec!(100.0);
        acc.locked = true;

        acc.deposit(dec!(50.0));
        assert_eq!(acc.available, dec!(150.0));

        assert!(!acc.withdraw(dec!(50.0)));
        assert_eq!(acc.available, dec!(150.0));

        assert!(!acc.hold(dec!(50.0)));
        assert_eq!(acc.available, dec!(150.0));
        assert_eq!(acc.held, dec!(0.0));

        acc.held = dec!(50.0);
        acc.available = dec!(100.0);
        acc.locked = true;

        assert!(!acc.release(dec!(50.0)));
        assert_eq!(acc.available, dec!(100.0));
        assert_eq!(acc.held, dec!(50.0));

        acc.held = dec!(50.0);
        assert!(acc.chargeback(dec!(50.0)));
        assert_eq!(acc.held, dec!(0.0));
        assert!(acc.locked);

        let mut acc2 = Account::new(2);
        acc2.held = dec!(30.0);
        acc2.locked = true;
        assert!(!acc2.chargeback(dec!(50.0)));
        assert_eq!(acc2.held, dec!(30.0));
    }

    #[rstest]
    #[case(1, dec!(10.0), dec!(10.0), dec!(0.0), false)]
    fn test_account_deposit(
        #[case] client_id: u16,
        #[case] deposit_amount: Decimal,
        #[case] expected_available: Decimal,
        #[case] expected_held: Decimal,
        #[case] expected_locked: bool,
    ) {
        let mut acc = Account::new(client_id);
        acc.deposit(deposit_amount);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
        assert_eq!(acc.locked, expected_locked);
        assert_eq!(acc.total(), expected_available + expected_held);
    }

    #[rstest]
    #[case(dec!(100.0), dec!(50.0), true, dec!(50.0))]
    #[case(dec!(100.0), dec!(100.0), true, dec!(0.0))]
    #[case(dec!(100.0), dec!(150.0), false, dec!(100.0))]
    fn test_account_withdraw(
        #[case] initial_available: Decimal,
        #[case] withdraw_amount: Decimal,
        #[case] expected_success: bool,
        #[case] expected_final_available: Decimal,
    ) {
        let mut acc = Account::new(1);
        acc.available = initial_available;
        let success = acc.withdraw(withdraw_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_final_available);
        assert_eq!(acc.held, dec!(0.0));
    }

    #[rstest]
    #[case(dec!(100.0), dec!(50.0), true, dec!(50.0), dec!(50.0))]
    #[case(dec!(100.0), dec!(150.0), false, dec!(100.0), dec!(0.0))]
    fn test_account_hold(
        #[case] initial_available: Decimal,
        #[case] hold_amount: Decimal,
        #[case] expected_success: bool,
        #[case] expected_available: Decimal,
        #[case] expected_held: Decimal,
    ) {
        let mut acc = Account::new(1);
        acc.available = initial_available;
        let success = acc.hold(hold_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
    }

    #[rstest]
    #[case(dec!(50.0), dec!(50.0), true, dec!(50.0), dec!(0.0))]
    #[case(dec!(50.0), dec!(100.0), false, dec!(0.0), dec!(50.0))]
    fn test_account_release(
        #[case] initial_held: Decimal,
        #[case] release_amount: Decimal,
        #[case] expected_success: bool,
        #[case] expected_available: Decimal,
        #[case] expected_held: Decimal,
    ) {
        let mut acc = Account::new(1);
        acc.held = initial_held;
        let success = acc.release(release_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
    }

    #[rstest]
    #[case(dec!(50.0), dec!(50.0), true, dec!(0.0), dec!(0.0), true)]
    #[case(dec!(50.0), dec!(100.0), false, dec!(0.0), dec!(50.0), false)]
    fn test_account_chargeback(
        #[case] initial_held: Decimal,
        #[case] chargeback_amount: Decimal,
        #[case] expected_success: bool,
        #[case] expected_available: Decimal,
        #[case] expected_held: Decimal,
        #[case] expected_locked: bool,
    ) {
        let mut acc = Account::new(1);
        acc.held = initial_held;
        let success = acc.chargeback(chargeback_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
        assert_eq!(acc.locked, expected_locked);
    }

    #[rstest]
    fn test_engine_deposit_and_withdraw() {
        let mut engine = PaymentEngine::new();
        let rec1 = InputRecord {
            record_type: TransactionType::Deposit,
            client_id: 1,
            tx_id: 1,
            amount: Some(dec!(100.0)),
        };
        let rec2 = InputRecord {
            record_type: TransactionType::Withdrawal,
            client_id: 1,
            tx_id: 2,
            amount: Some(dec!(30.0)),
        };
        let rec3 = InputRecord {
            record_type: TransactionType::Withdrawal,
            client_id: 1,
            tx_id: 3,
            amount: Some(dec!(80.0)),
        }; // Should fail

        assert!(engine.process(rec1).is_ok());
        assert!(engine.process(rec2).is_ok());
        assert!(engine.process(rec3).is_ok());

        let acc = engine.accounts.get(&1).unwrap();
        assert_eq!(acc.available, dec!(70.0));
        assert_eq!(acc.held, dec!(0.0));
        assert!(!acc.locked);
        assert_eq!(engine.transactions.len(), 1);
    }

    #[rstest]
    fn test_engine_full_dispute_cycle() {
        let mut engine = PaymentEngine::new();
        engine
            .process(InputRecord {
                record_type: TransactionType::Deposit,
                client_id: 1,
                tx_id: 1,
                amount: Some(dec!(100.0)),
            })
            .unwrap();

        engine
            .process(InputRecord {
                record_type: TransactionType::Dispute,
                client_id: 1,
                tx_id: 1,
                amount: None,
            })
            .unwrap();
        let acc1 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc1.available, dec!(0.0));
        assert_eq!(acc1.held, dec!(100.0));
        assert_eq!(
            engine.transactions.get(&1).unwrap().state,
            TransactionState::Disputed
        );

        engine
            .process(InputRecord {
                record_type: TransactionType::Resolve,
                client_id: 1,
                tx_id: 1,
                amount: None,
            })
            .unwrap();
        let acc2 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc2.available, dec!(100.0));
        assert_eq!(acc2.held, dec!(0.0));
        assert!(!acc2.locked);
        // the transaction is *gone* after being resolved
        assert!(!engine.transactions.contains_key(&1));
    }

    #[rstest]
    fn test_engine_dispute_chargeback() {
        let mut engine = PaymentEngine::new();
        engine
            .process(InputRecord {
                record_type: TransactionType::Deposit,
                client_id: 1,
                tx_id: 1,
                amount: Some(dec!(100.0)),
            })
            .unwrap();

        engine
            .process(InputRecord {
                record_type: TransactionType::Dispute,
                client_id: 1,
                tx_id: 1,
                amount: None,
            })
            .unwrap();
        let acc1 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc1.available, dec!(0.0));
        assert_eq!(acc1.held, dec!(100.0));

        engine
            .process(InputRecord {
                record_type: TransactionType::Chargeback,
                client_id: 1,
                tx_id: 1,
                amount: None,
            })
            .unwrap();
        let acc2 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc2.available, dec!(0.0));
        assert_eq!(acc2.held, dec!(0.0));
        assert!(acc2.locked); // Account is now locked
                              // the transaction is *gone* after being resolved
        assert!(!engine.transactions.contains_key(&1));
    }

    #[rstest]
    #[case(TransactionType::Dispute)]
    #[case(TransactionType::Resolve)]
    #[case(TransactionType::Chargeback)]
    fn test_engine_ignore_non_existent_tx(#[case] tx_type: TransactionType) {
        let mut engine = PaymentEngine::new();
        let record = InputRecord {
            record_type: tx_type,
            client_id: 1,
            tx_id: 99,
            amount: None,
        };

        assert!(engine.process(record).is_ok());
        assert!(engine.accounts.is_empty());
        assert!(engine.transactions.is_empty());
    }

    #[rstest]
    #[case(TransactionType::Resolve)]
    #[case(TransactionType::Chargeback)]
    fn test_engine_ignore_invalid_state(#[case] tx_type: TransactionType) {
        let mut engine = PaymentEngine::new();
        engine
            .process(InputRecord {
                record_type: TransactionType::Deposit,
                client_id: 1,
                tx_id: 1,
                amount: Some(dec!(100.0)),
            })
            .unwrap();

        let record = InputRecord {
            record_type: tx_type,
            client_id: 1,
            tx_id: 1,
            amount: None,
        };
        assert!(engine.process(record).is_ok());

        let acc = engine.accounts.get(&1).unwrap();
        assert_eq!(acc.available, dec!(100.0));
        assert_eq!(acc.held, dec!(0.0));
        assert_eq!(
            engine.transactions.get(&1).unwrap().state,
            TransactionState::Normal
        );
    }

    #[rstest]
    fn test_engine_ignore_dispute_already_disputed() {
        let mut engine = PaymentEngine::new();
        engine
            .process(InputRecord {
                record_type: TransactionType::Deposit,
                client_id: 1,
                tx_id: 1,
                amount: Some(dec!(100.0)),
            })
            .unwrap();
        engine
            .process(InputRecord {
                record_type: TransactionType::Dispute,
                client_id: 1,
                tx_id: 1,
                amount: None,
            })
            .unwrap();

        let acc_before = engine.accounts.get(&1).unwrap().clone();
        let tx_state_before = engine.transactions.get(&1).unwrap().state;

        engine
            .process(InputRecord {
                record_type: TransactionType::Dispute,
                client_id: 1,
                tx_id: 1,
                amount: None,
            })
            .unwrap();

        let acc_after = engine.accounts.get(&1).unwrap();
        let tx_state_after = engine.transactions.get(&1).unwrap().state;

        assert_eq!(&acc_before, acc_after);
        assert_eq!(tx_state_before, tx_state_after);
    }

    #[rstest]
    fn test_engine_deposit_missing_amount() {
        let mut engine = PaymentEngine::new();
        let record = InputRecord {
            record_type: TransactionType::Deposit,
            client_id: 1,
            tx_id: 99,
            amount: None,
        };

        let result = engine.process(record);

        assert!(result.is_err());

        match result.err().unwrap() {
            PaymentError::InvalidTransaction(msg) => {
                assert!(msg.contains("Deposit 99 missing amount"));
            }
            _ => panic!("Expected InvalidTransaction error"),
        }
        assert!(engine.accounts.is_empty());
    }

    #[rstest]
    #[case(dec!(-10.0))]
    #[case(dec!(0.0))]
    fn test_engine_deposit_invalid_amount(#[case] invalid_amount: Decimal) {
        let mut engine = PaymentEngine::new();
        let record = InputRecord {
            record_type: TransactionType::Deposit,
            client_id: 1,
            tx_id: 100,
            amount: Some(invalid_amount),
        };

        let result = engine.process(record);

        assert!(result.is_err());

        match result.err().unwrap() {
            PaymentError::InvalidTransaction(msg) => {
                assert!(msg.contains("Deposit amount for tx 100 must be positive"));
            }
            _ => panic!("Expected InvalidTransaction error"),
        }
        assert!(engine.accounts.is_empty());
    }

    #[rstest]
    fn test_engine_withdrawal_missing_amount() {
        let mut engine = PaymentEngine::new();
        let record = InputRecord {
            record_type: TransactionType::Withdrawal,
            client_id: 1,
            tx_id: 201,
            amount: None,
        };

        let result = engine.process(record);

        assert!(result.is_err());

        match result.err().unwrap() {
            PaymentError::InvalidTransaction(msg) => {
                assert!(msg.contains("Withdrawal 201 missing amount"));
            }
            _ => panic!("Expected InvalidTransaction error"),
        }
        assert!(engine.accounts.is_empty());
    }

    #[rstest]
    fn test_engine_duplicate_deposit_is_ignored() {
        let mut engine = PaymentEngine::new();
        let record = InputRecord {
            record_type: TransactionType::Deposit,
            client_id: 1,
            tx_id: 1,
            amount: Some(rust_decimal_macros::dec!(100.0)),
        };

        // First deposit should be processed
        assert!(engine.process(record.clone()).is_ok());

        // Second deposit with same tx_id should be ignored (covered line)
        assert!(engine.process(record.clone()).is_ok());

        // Only one deposit should be reflected in the account
        let acc = engine.accounts.get(&1).unwrap();
        assert_eq!(acc.available, rust_decimal_macros::dec!(100.0));
        assert_eq!(engine.transactions.len(), 1);
    }

    #[rstest]
    #[case(TransactionType::Dispute, 42, 99, TransactionState::Normal, dec!(10.0))]
    #[case(TransactionType::Resolve, 55, 77, TransactionState::Disputed, dec!(25.0))]
    #[case(TransactionType::Chargeback, 88, 123, TransactionState::Disputed, dec!(40.0))]
    fn test_missing_account_is_ignored(
        #[case] tx_type: TransactionType,
        #[case] tx_id: u32,
        #[case] client_id: u16,
        #[case] state: TransactionState,
        #[case] amount: Decimal,
    ) {
        let mut engine = PaymentEngine::new();

        // Insert a transaction for a client that does not exist in accounts
        engine.transactions.insert(
            tx_id,
            TransactionInfo {
                client_id,
                amount,
                state,
            },
        );

        // Now process the record (dispute/resolve/chargeback)
        let record = InputRecord {
            record_type: tx_type,
            client_id,
            tx_id,
            amount: None,
        };

        // This should hit the `None => return Ok(())` branch
        assert!(engine.process(record).is_ok());
        // Still no account created
        assert!(!engine.accounts.contains_key(&client_id));
    }

    #[rstest]
    #[case(dec!(-10.0))]
    #[case(dec!(0.0))]
    fn test_engine_withdrawal_invalid_amount(#[case] invalid_amount: Decimal) {
        let mut engine = PaymentEngine::new();
        let record = InputRecord {
            record_type: TransactionType::Withdrawal,
            client_id: 1,
            tx_id: 202,
            amount: Some(invalid_amount),
        };

        let result = engine.process(record);

        assert!(result.is_err());

        match result.err().unwrap() {
            PaymentError::InvalidTransaction(msg) => {
                assert!(msg.contains("Withdrawal amount for tx 202 must be positive"));
            }
            _ => panic!("Expected InvalidTransaction error"),
        }
        assert!(engine.accounts.is_empty());
    }
}
