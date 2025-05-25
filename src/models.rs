use rust_decimal::Decimal;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Debug, Deserialize, Clone)]
pub struct InputRecord {
    #[serde(rename = "type")]
    pub record_type: TransactionType,
    #[serde(rename = "client")]
    pub client_id: u16,
    #[serde(rename = "tx")]
    pub tx_id: u32,
    pub amount: Option<Decimal>,
}

#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct OutputRecord {
    #[serde(rename = "client")]
    pub client_id: u16,
    pub available: Decimal,
    pub held: Decimal,
    pub total: Decimal,
    pub locked: bool,
}

#[derive(Debug, PartialEq, Clone)]
pub struct Account {
    pub client_id: u16,
    pub available: Decimal,
    pub held: Decimal,
    pub locked: bool,
}

impl Account {
    pub fn new(client_id: u16) -> Self {
        Account {
            client_id,
            available: Decimal::new(0, 4),
            held: Decimal::new(0, 4),
            locked: false,
        }
    }

    pub fn total(&self) -> Decimal {
        self.available + self.held
    }

    /// Processes a deposit into the account.
    pub fn deposit(&mut self, amount: Decimal) {
        self.available += amount;
    }

    /// Processes a withdrawal from the account.
    /// Returns true if successful, false otherwise (insufficient funds or locked).
    pub fn withdraw(&mut self, amount: Decimal) -> bool {
        if !self.locked && self.available >= amount {
            self.available -= amount;
            true
        } else {
            false
        }
    }

    /// Puts funds on hold due to a dispute.
    pub fn hold(&mut self, amount: Decimal) -> bool {
        if !self.locked && self.available >= amount {
            self.available -= amount;
            self.held += amount;
            true
        } else {
            false
        }
    }

    /// Releases held funds after a dispute resolution.
    pub fn release(&mut self, amount: Decimal) -> bool {
        if !self.locked && self.held >= amount {
            self.held -= amount;
            self.available += amount;
            true
        } else {
            false
        }
    }

    /// Processes a chargeback, removing held funds and locking the account.
    pub fn chargeback(&mut self, amount: Decimal) -> bool {
        if self.held >= amount {
            self.held -= amount;
            self.locked = true;
            true
        } else {
            false
        }
    }

    pub fn to_output_record(&self) -> OutputRecord {
        OutputRecord {
            client_id: self.client_id,
            available: self.available,
            held: self.held,
            total: self.total(),
            locked: self.locked,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TransactionState {
    Normal,
    Disputed,
}

#[derive(Debug, Clone, Copy)]
pub struct TransactionInfo {
    pub client_id: u16,
    pub amount: Decimal,
    pub state: TransactionState,
}
