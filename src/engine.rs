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
    /// Creates a new `PaymentEngine`.
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
        if matches!(record.record_type, TransactionType::Deposit | TransactionType::Withdrawal)
            && self.transactions.contains_key(&tx_id) {
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

    /// Handles a deposit transaction.
    fn handle_deposit(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let amount = record.amount.ok_or_else(|| {
            PaymentError::InvalidTransaction(format!("Deposit {} missing amount", record.tx_id))
        })?;
        if amount <= Decimal::ZERO {
             return Err(PaymentError::InvalidTransaction(format!("Deposit amount for tx {} must be positive", record.tx_id)));
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

    /// Handles a withdrawal transaction.
    fn handle_withdrawal(&mut self, record: InputRecord) -> Result<(), PaymentError> {
        let amount = record.amount.ok_or_else(|| {
            PaymentError::InvalidTransaction(format!("Withdrawal {} missing amount", record.tx_id))
        })?;
         if amount <= Decimal::ZERO {
             return Err(PaymentError::InvalidTransaction(format!("Withdrawal amount for tx {} must be positive", record.tx_id)));
        }

        let account = self.get_or_create_account(record.client_id);
        // account.withdraw will check for locked status.
        account.withdraw(amount); // We ignore the bool result as per spec.
        Ok(())
    }

    /// Handles a dispute transaction.
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
            // Now get mutable ref to update state
            if let Some(tx_to_update) = self.transactions.get_mut(&tx_id) {
                tx_to_update.state = TransactionState::Disputed;
            }
        }
        Ok(())
    }

    /// Handles a resolve transaction.
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
             if let Some(tx_to_update) = self.transactions.get_mut(&tx_id) {
                tx_to_update.state = TransactionState::Normal;
             }
        }
        Ok(())
    }

    /// Handles a chargeback transaction.
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

        // Account::chargeback now handles held funds and locking.
        if account.chargeback(tx_info.amount) {
            if let Some(tx_to_update) = self.transactions.get_mut(&tx_id) {
                tx_to_update.state = TransactionState::ChargedBack;
            }
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

// --- Tests ---
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, TransactionType};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    #[test]
    fn test_locked_account_behavior() {
        let mut acc = Account::new(1);
        acc.available = dec!(100.0);
        acc.locked = true;

        // Deposit should now work
        acc.deposit(dec!(50.0));
        assert_eq!(acc.available, dec!(150.0)); // <<< This should now pass

        // Withdraw should fail
        assert!(!acc.withdraw(dec!(50.0)));
        assert_eq!(acc.available, dec!(150.0));

        // Hold should fail
        assert!(!acc.hold(dec!(50.0)));
        assert_eq!(acc.available, dec!(150.0));
        assert_eq!(acc.held, dec!(0.0));

        acc.held = dec!(50.0); // Manually set held for release/chargeback test
        acc.available = dec!(100.0); // Reset available
        acc.locked = true; // Ensure it stays locked for these tests

        // Release should fail
        assert!(!acc.release(dec!(50.0)));
        assert_eq!(acc.available, dec!(100.0));
        assert_eq!(acc.held, dec!(50.0));

        // Chargeback should *succeed* (per our updated logic) but not change held if amount > held
        // Let's test success case
        acc.held = dec!(50.0);
        assert!(acc.chargeback(dec!(50.0))); // <<< This now succeeds
        assert_eq!(acc.held, dec!(0.0)); // Held decreases
        assert!(acc.locked); // Stays locked

        // Test fail case (insufficient held)
        let mut acc2 = Account::new(2);
        acc2.held = dec!(30.0);
        acc2.locked = true;
        assert!(!acc2.chargeback(dec!(50.0))); // Fails because held < amount
        assert_eq!(acc2.held, dec!(30.0));
    }


    // --- Account Tests ---
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
    #[case(dec!(100.0), dec!(50.0), true, dec!(50.0))] // Success
    #[case(dec!(100.0), dec!(100.0), true, dec!(0.0))] // Success exact
    #[case(dec!(100.0), dec!(150.0), false, dec!(100.0))] // Fail insufficient
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
    #[case(dec!(100.0), dec!(50.0), true, dec!(50.0), dec!(50.0))] // Success
    #[case(dec!(100.0), dec!(150.0), false, dec!(100.0), dec!(0.0))] // Fail insufficient
    fn test_account_hold(
         #[case] initial_available: Decimal,
         #[case] hold_amount: Decimal,
         #[case] expected_success: bool,
         #[case] expected_available: Decimal,
         #[case] expected_held: Decimal,
    ){
        let mut acc = Account::new(1);
        acc.available = initial_available;
        let success = acc.hold(hold_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
    }

     #[rstest]
    #[case(dec!(50.0), dec!(50.0), true, dec!(50.0), dec!(0.0))] // Success
    #[case(dec!(50.0), dec!(100.0), false, dec!(0.0), dec!(50.0))] // Fail insufficient
    fn test_account_release(
         #[case] initial_held: Decimal,
         #[case] release_amount: Decimal,
         #[case] expected_success: bool,
         #[case] expected_available: Decimal,
         #[case] expected_held: Decimal,
    ){
        let mut acc = Account::new(1);
        acc.held = initial_held;
        let success = acc.release(release_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
    }

     #[rstest]
    #[case(dec!(50.0), dec!(50.0), true, dec!(0.0), dec!(0.0), true)] // Success
    #[case(dec!(50.0), dec!(100.0), false, dec!(0.0), dec!(50.0), false)] // Fail insufficient
    fn test_account_chargeback(
         #[case] initial_held: Decimal,
         #[case] chargeback_amount: Decimal,
         #[case] expected_success: bool,
         #[case] expected_available: Decimal,
         #[case] expected_held: Decimal,
         #[case] expected_locked: bool,
    ){
        let mut acc = Account::new(1);
        acc.held = initial_held;
        let success = acc.chargeback(chargeback_amount);
        assert_eq!(success, expected_success);
        assert_eq!(acc.available, expected_available);
        assert_eq!(acc.held, expected_held);
        assert_eq!(acc.locked, expected_locked);
    }

    // --- Engine Tests ---
    #[test]
    fn test_engine_deposit_and_withdraw() {
        let mut engine = PaymentEngine::new();
        let rec1 = InputRecord { record_type: TransactionType::Deposit, client_id: 1, tx_id: 1, amount: Some(dec!(100.0)) };
        let rec2 = InputRecord { record_type: TransactionType::Withdrawal, client_id: 1, tx_id: 2, amount: Some(dec!(30.0)) };
        let rec3 = InputRecord { record_type: TransactionType::Withdrawal, client_id: 1, tx_id: 3, amount: Some(dec!(80.0)) }; // Should fail

        assert!(engine.process(rec1).is_ok());
        assert!(engine.process(rec2).is_ok());
        assert!(engine.process(rec3).is_ok());

        let acc = engine.accounts.get(&1).unwrap();
        assert_eq!(acc.available, dec!(70.0));
        assert_eq!(acc.held, dec!(0.0));
        assert!(!acc.locked);
        assert_eq!(engine.transactions.len(), 1); // Only deposit stored
    }

     #[test]
    fn test_engine_full_dispute_cycle() {
        let mut engine = PaymentEngine::new();
        engine.process(InputRecord { record_type: TransactionType::Deposit, client_id: 1, tx_id: 1, amount: Some(dec!(100.0)) }).unwrap();

        // Dispute
        engine.process(InputRecord { record_type: TransactionType::Dispute, client_id: 1, tx_id: 1, amount: None }).unwrap();
        let acc1 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc1.available, dec!(0.0));
        assert_eq!(acc1.held, dec!(100.0));
        assert_eq!(engine.transactions.get(&1).unwrap().state, TransactionState::Disputed);

        // Resolve
        engine.process(InputRecord { record_type: TransactionType::Resolve, client_id: 1, tx_id: 1, amount: None }).unwrap();
        let acc2 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc2.available, dec!(100.0));
        assert_eq!(acc2.held, dec!(0.0));
         assert!(!acc2.locked);
        assert_eq!(engine.transactions.get(&1).unwrap().state, TransactionState::Normal);
    }

     #[test]
    fn test_engine_dispute_chargeback() {
        let mut engine = PaymentEngine::new();
        engine.process(InputRecord { record_type: TransactionType::Deposit, client_id: 1, tx_id: 1, amount: Some(dec!(100.0)) }).unwrap();

        // Dispute
        engine.process(InputRecord { record_type: TransactionType::Dispute, client_id: 1, tx_id: 1, amount: None }).unwrap();
        let acc1 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc1.available, dec!(0.0));
        assert_eq!(acc1.held, dec!(100.0));

        // Chargeback
        engine.process(InputRecord { record_type: TransactionType::Chargeback, client_id: 1, tx_id: 1, amount: None }).unwrap();
        let acc2 = engine.accounts.get(&1).unwrap();
        assert_eq!(acc2.available, dec!(0.0));
        assert_eq!(acc2.held, dec!(0.0));
        assert!(acc2.locked); // Account is now locked
        assert_eq!(engine.transactions.get(&1).unwrap().state, TransactionState::ChargedBack);
    }

    #[rstest]
    #[case(TransactionType::Dispute)] // Non-existent dispute
    #[case(TransactionType::Resolve)] // Non-existent resolve
    #[case(TransactionType::Chargeback)] // Non-existent chargeback
    fn test_engine_ignore_non_existent_tx(#[case] tx_type: TransactionType) {
        let mut engine = PaymentEngine::new();
        let record = InputRecord { record_type: tx_type, client_id: 1, tx_id: 99, amount: None };

        assert!(engine.process(record).is_ok());
        assert!(engine.accounts.is_empty()); // No account created based on ignored TX
        assert!(engine.transactions.is_empty());
    }

    #[rstest]
    #[case(TransactionType::Resolve)] // Resolve non-disputed
    #[case(TransactionType::Chargeback)] // Chargeback non-disputed
    fn test_engine_ignore_invalid_state(#[case] tx_type: TransactionType) {
         let mut engine = PaymentEngine::new();
         engine.process(InputRecord { record_type: TransactionType::Deposit, client_id: 1, tx_id: 1, amount: Some(dec!(100.0)) }).unwrap();

         let record = InputRecord { record_type: tx_type, client_id: 1, tx_id: 1, amount: None };
         assert!(engine.process(record).is_ok());

         let acc = engine.accounts.get(&1).unwrap();
         assert_eq!(acc.available, dec!(100.0)); // No change
         assert_eq!(acc.held, dec!(0.0)); // No change
         assert_eq!(engine.transactions.get(&1).unwrap().state, TransactionState::Normal); // No change
     }

     #[test]
     fn test_engine_ignore_dispute_already_disputed() {
         let mut engine = PaymentEngine::new();
         engine.process(InputRecord { record_type: TransactionType::Deposit, client_id: 1, tx_id: 1, amount: Some(dec!(100.0)) }).unwrap();
         engine.process(InputRecord { record_type: TransactionType::Dispute, client_id: 1, tx_id: 1, amount: None }).unwrap();

         let acc_before = engine.accounts.get(&1).unwrap().clone();
         let tx_state_before = engine.transactions.get(&1).unwrap().state;

         // Process second dispute
         engine.process(InputRecord { record_type: TransactionType::Dispute, client_id: 1, tx_id: 1, amount: None }).unwrap();

         let acc_after = engine.accounts.get(&1).unwrap();
         let tx_state_after = engine.transactions.get(&1).unwrap().state;

         assert_eq!(&acc_before, acc_after); // No change
         assert_eq!(tx_state_before, tx_state_after); // No change
     }
}
