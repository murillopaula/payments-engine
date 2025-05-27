![CI](https://github.com/murillopaula/payments-engine/actions/workflows/rust.yml/badge.svg)
[![codecov](https://codecov.io/gh/murillopaula/payments-engine/graph/badge.svg?token=C8Z8IE203Q)](https://codecov.io/gh/murillopaula/payments-engine)
![Security Audit](https://img.shields.io/github/actions/workflow/status/murillopaula/payments-engine/security.yml?label=security)
[![dependency status](https://deps.rs/repo/github/murillopaula/payments-engine/status.svg)](https://deps.rs/repo/github/murillopaula/payments-engine)

# Payment Engine

A streaming transaction processor that handles deposits, withdrawals, and the complete dispute lifecycle (dispute → resolve/chargeback). Built with Rust for reliability and performance.

## Quick Start

```bash
cargo run -- transactions.csv > accounts.csv
```

That's it. The engine reads transactions from a CSV file and outputs final account states.

## Core Design

The engine processes transactions one at a time without loading the entire dataset into memory. Key design choices:

- Only client accounts and disputable deposits are kept in memory. Once a transaction is resolved or charged back, it's removed. This lets us handle billions of transactions with minimal RAM.

- After some deliberation, I decided that only deposits can be disputed (withdrawals are client-initiated actions). Disputes require sufficient available funds - if you've already spent the money, you can't put it on hold.

- Bad records are logged and skipped. The engine keeps processing. No panics, no stopping on the first error. This matches real payment systems that must be resilient to bad data.

### Key Assumptions

I made these decisions:

1. Only deposits can be disputed (makes sense from a banking perspective)
2. Disputes need available funds (can't hold money that's already spent)
3. Resolved/charged-back transactions are final (removed from memory)
4. CSV format can vary (trailing commas, whitespace) - handled flexibly
5. Output precision matches examples (minimal decimal places)

## Architecture

```
Input CSV → Streaming Parser → Payment Engine → Output CSV
                                      ↓
                             Account State (HashMap)
                             Transaction Store (HashMap)
```

The modular structure makes each component testable in isolation:
- `engine.rs` - Core business logic and state management
- `csv_handler.rs` - Streaming CSV I/O
- `models.rs` - Domain types with serde integration
- `errors.rs` - Error types using thiserror

## Usage

Build and run:
```bash
# Development
cargo run -- input.csv > output.csv

# Production (optimized)
cargo build --release
./target/release/payment_engine input.csv > output.csv
```

Input format:
```csv
type,client,tx,amount
deposit,1,1,100.0
withdrawal,1,2,50.0
dispute,1,1,
```

Output format:
```csv
client,available,held,total,locked
1,50.0,0.0,50.0,false
```

## Testing Strategy

The test suite covers unit tests, integration tests, and edge cases:

```bash
# Run all tests
cargo test

# Run integration tests with test data
./tests/integration_tests.sh

# Run with coverage report
cargo llvm-cov --html
```

### Test Structure

**Unit tests** (`src/*.rs`): Each module has embedded tests covering individual functions and error paths. Used `rstest` for parameterized testing to avoid repetition.

**CLI tests** (`tests/cli.rs`): End-to-end testing of the binary, including error cases like missing files and write failures.

**Test data** (`tests/data/*.csv`): Real-world scenarios with expected outputs:
- Basic transactions
- Full dispute cycles
- Invalid operations
- Edge cases (locked accounts, insufficient funds)
- CSV format variations

### Coverage

Current coverage sits at 100% with all paths tested.

## Performance

- Memory usage: O(clients + active_disputes)
- Time Complexity: O(N) where N = total number of transactions in the CSV

### Optimizations Done

1. **Transaction pruning**: Resolved/charged-back transactions are removed immediately
2. **Efficient parsing**: Using `csv` crate with minimal allocations
3. **Simple data structures**: HashMaps provide O(1) lookups
4. **Zero-copy where possible**: Decimal parsing without intermediate strings

### Potential Future Enhancements (Hypothetical, if scaling further or for server use):

1. **Disk-Based Transaction Storage**: For truly massive `u32` scale transaction histories, the `transactions` map could be moved to an embedded, disk-based key-value store (e.g., `sled`). This would keep RAM usage for transactions minimal, at the cost of slower disk I/O for lookups. `TransactionInfo` would need to be (de)serializable (e.g., using `bincode`).

2. **Concurrency & Asynchronous I/O**: If the engine were to be part of a server handling thousands of concurrent TCP streams, an asynchronous architecture using` tokio` or `async-std` would be necessary.

    - I/O operations (reading from streams, writing responses) would be async.
    - The PaymentEngine itself, or at least access to its shared data (accounts, transaction store), would need to be made thread-safe, likely using `Arc<tokio::sync::Mutex<PaymentEngine>>` or similar concurrent data structures and patterns.

## Development Process

### Code Quality

```bash
# Format code
cargo fmt

# Run cargo check
cargo check --locked

# Run linter
cargo clippy --locked --all-targets -- -D warnings

# Security audit
cargo audit
```

### CI Pipeline

The project includes GitHub Actions configuration for running unit, CLI and integration tests on each push, code coverage reporting, clippy linting, cargo check, and security audits.

### Dependencies

Minimal, audited dependencies:
- `csv` - Battle-tested CSV parsing
- `serde` - Standard serialization
- `rust_decimal` - Accurate financial math
- `thiserror` - Ergonomic error handling

## Implementation Details

### Transaction Rules

- Deposits
    * Credit the account. Store transaction for potential disputes.

- Withdrawals
    * Debit if funds available and account not locked. Fail silently otherwise.

- Disputes
    * Move funds from available to held if sufficient balance exists. Mark transaction as disputed.

- Resolves
    * Return held funds to available. Remove transaction from memory (can't be disputed again).

- Chargebacks
    * Remove held funds, lock account. Remove transaction from memory.

### Edge Cases Handled

- Duplicate transaction IDs are ignored
- Disputing non-existent transactions is ignored
- Negative amounts trigger errors (logged to stderr)
- Locked accounts can receive deposits but not withdraw
- Double disputes on same transaction are ignored
- Withdrawals can't be disputed
