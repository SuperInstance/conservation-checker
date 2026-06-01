# conservation-checker

**One-sided conservation laws for real systems.**

Track quantities that must not decrease across operations — budgets, energy, quotas, token counts, throughput — with tolerance, drift detection, and phase analysis.

[![crates.io](https://img.shields.io/crates/v/conservation-checker.svg)](https://crates.io/crates/conservation-checker)
[![docs.rs](https://docs.rs/conservation-checker/badge.svg)](https://docs.rs/conservation-checker)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## 30-Second Example

```rust
use conservation_checker::ConservationChecker;

let mut checker = ConservationChecker::new();

// Register a quantity: name, initial value, allowed decrease (tolerance)
checker.register("energy", 100.0, 5.0);

// Update as your system runs
checker.update("energy", 98.0);  // still conserved
checker.snapshot();

checker.update("energy", 90.0);  // dropped past tolerance!
checker.snapshot();

assert!(!checker.is_conserved("energy"));
assert_eq!(checker.violations(), vec!["energy"]);
```

## Why?

Conservation laws aren't just physics. Real systems have quantities that should only increase or stay flat:

- **Budget tracking** — spending must not exceed allocation
- **API rate limits** — request count must not exceed quota
- **Energy monitoring** — battery drain beyond expected range signals problems
- **Token budgets** — LLM token consumption against a cap
- **Throughput quotas** — data transfer against a monthly limit

`conservation-checker` gives you a simple, zero-dependency way to assert these invariants, detect when they're violated, and track trends over time.

## Real Use Cases

### Budget Tracking

```rust
let mut budget = ConservationChecker::new();
budget.register("monthly", 5000.0, 100.0); // $100 buffer

for expense in &expenses {
    let remaining = budget.current_value("monthly") - expense;
    budget.update("monthly", remaining);
    budget.snapshot();
}

if !budget.violations().is_empty() {
    println!("⚠️ Over budget!");
}
```

### API Rate Limits

```rust
let mut limits = ConservationChecker::new();
limits.register("requests_remaining", 10000.0, 0.0);

// After each API call:
limits.update("requests_remaining", remaining_count);
limits.snapshot();

if limits.phase("requests_remaining") == Phase::Transitioning {
    println!("Rate limit approaching — slow down!");
}
```

### Energy Monitoring

```rust
let mut monitor = ConservationChecker::new();
monitor.register("battery", 100.0, 20.0); // 20% tolerance

monitor.update("battery", current_level);
monitor.snapshot();

let drain = monitor.drift_rate("battery"); // % per tick
println!("Battery drain: {:.1}%/tick", drain);
```

### Token Budgets

```rust
let mut tokens = ConservationChecker::new();
tokens.register("token_cap", 4096.0, 0.0);

tokens.update("token_cap", tokens_used);
if !tokens.is_conserved("token_cap") {
    return Err("Token budget exceeded!");
}
```

## API Reference

### `ConservationChecker`

| Method | Description |
|--------|-------------|
| `new()` | Create an empty tracker |
| `register(name, initial, tolerance)` | Register a quantity with initial value and allowed decrease |
| `update(name, value)` | Set the current value of a quantity |
| `is_conserved(name) -> bool` | Check if value ≥ initial − tolerance |
| `violations() -> Vec<String>` | List all quantities currently violated |
| `snapshot()` | Record current values into history |
| `phase(name) -> Phase` | Detect trajectory phase from history |
| `drift_rate(name) -> f64` | Average change per snapshot |
| `current_value(name) -> f64` | Get the current value |
| `initial_value(name) -> f64` | Get the initial (baseline) value |
| `snapshot_count(name) -> usize` | Number of snapshots recorded |
| `registered() -> Vec<String>` | All registered quantity names |
| `deregister(name) -> bool` | Remove a quantity |
| `reset_baseline(name)` | Reset initial value to current, clearing violations |

### `Phase`

```rust
pub enum Phase {
    Stable,          // No significant change
    PreTransition,   // Accelerating but not yet violated
    Transitioning,   // Actively decreasing past tolerance
    Resolving,       // Was violated, now recovering
}
```

## Design Principles

- **Zero dependencies** — nothing to audit, nothing to break
- **`#![deny(unsafe_code)]`** — memory safety guaranteed
- **One-sided** — increases are always OK, only decreases can violate
- **Tolerance-aware** — real systems have acceptable ranges, not exact values
- **Time-series built in** — snapshot history enables drift and phase detection

## Running Examples

```bash
cargo run --example basic
cargo run --example budget_tracking
```

## License

MIT
