# TEST-DIARY.md — conservation-checker v0.2.0

**Tester:** Kenji (SRE, cloud infra)
**Date:** 2026-06-01
**Repo:** SuperInstance/conservation-checker
**Test scope:** Full source review, build, unit tests, examples, custom SRE cluster monitoring scenario

---

## 1. First Impression — README Clarity

**Score: 4/5**

The README is well-structured and immediately explains the crate's purpose: one-sided conservation laws for real systems. The 30-second example is genuinely 30 seconds — copy, paste, run. The "Why?" section anchors the concept in relatable SRE concerns (budgets, rate limits, energy, tokens, throughput).

The "When to Use This vs negative-space-testing" callout is good hygiene — saves confusion for anyone coming from the testing ecosystem.

**Minor nitpicks:**
- README claims "zero dependencies" but the `serde` feature pulls in `serde` + `serde_json` (admittedly optional, but worth flagging)
- The API reference table is nice but `snapshot_json()` (only available with `serde` feature) isn't listed there — it appears only in the serde section of lib.rs
- No diagram or visual for the Phase state machine. The text comment in the source (`Stable → PreTransition → Transitioning → Resolving → Stable`) would be great in the README too

---

## 2. Build — Does It Compile? Warnings?

**Build (default features):** ✅ Clean. 0 warnings.
**Build (serde feature):** ✅ Clean. 0 warnings.
**Examples (basic, budget_tracking):** ✅ Run without issues.

**Unit tests:** 53 passed, 6 doc-tests passed. All green.

The crate has no `unsafe` code (`#![deny(unsafe_code)]`) and no external dependencies in the default build. This is genuinely impressive for a production monitoring crate — minimal audit surface is a big plus for SRE adoption.

---

## 3. API — What Conservation Laws Does It Support?

After reading `src/lib.rs` in full:

### Core concept: one-sided conservation

The crate enforces that a quantity **must not decrease** past a tolerance threshold. Increases are always OK. This maps naturally to:

| SRE Use Case | How It Maps |
|---|---|
| CPU budget remaining | Must not decrease below 30% |
| Memory remaining | Must not decrease (invert: 64GB - used) |
| Network throughput | Must not drop below minimum Mbps |
| Disk space remaining | Must not decrease |
| Rate limit quota | Must not decrease below zero |
| Token budget | Must not decrease below cap |
| Battery charge | Must not drop faster than expected |

### Phase detection (the star feature)

The `Phase` enum is genuinely useful for SRE — it doesn't just tell you "yes/no violated", it tells you the **trajectory**:

- **Stable** — fine, no significant change
- **PreTransition** — accelerating downward but not violated yet (early warning!)
- **Transitioning** — actively violating
- **Resolving** — was bad, recovering

The phase detection works by comparing short-term rates vs older rates against a noise floor derived from tolerance. It's simple but effective in practice.

### Key API surface

- `register(name, initial, tolerance)` — set up a conservation law
- `update(name, value)` — push a new value
- `snapshot()` — record all current values into time-series history
- `is_conserved(name) -> bool`
- `violations() -> Vec<String>`
- `phase(name) -> Phase` — early warning system
- `drift_rate(name) -> f64` — average change per snapshot
- `reset_baseline(name)` — reset initial to current (useful for post-incident recovery)
- `snapshot_json()` — **serde feature** — serializes current state to JSON

### What's missing (see also section 5)

- Two-sided conservation (must not exceed upper bound) — only possible via inversion trick
- Annotations/metadata on quantities (why was 30% chosen as the threshold?)
- Bulk operations (register 10 nodes in one call)

---

## 4. Real Test — SRE Cluster Monitoring Scenario

I created and ran `examples/cluster_monitoring.rs`: a 10-node cluster (web, db, worker, cache, monitor) running 20 monitoring ticks with simulated resource fluctuations.

### Setup

- **10 nodes** with CPU %, memory GB, and network Mbps per node
- **Conservation laws:** CPU ≥ 30%, memory ≤ 48 GB (inverted to "memory remaining ≥ 16 GB"), network ≥ 10 Mbps
- **30 registered quantities** (3 per node × 10 nodes)
- **Deterministic simulation** (seeded by tick × node index)
- **Serde snapshot** at the end for archival

### Findings

The conservation checker caught real violations:

- **CPU alerts:** Multiple nodes dipped below 30% (web-02@16%, db-01@11% at worst)
- **Memory alerts:** All 10 nodes exceeded the 16GB remaining threshold by tick 10
- **Network:** Remained healthy (the bursty pattern stayed above 10 Mbps)
- **Phase detection** correctly identified Transitioning nodes (actively dropping), PreTransition (early warning on network), and Resolving (nodes recovering from dips)

### What phase detection showed

The phase data was actually useful:
- `db-01.cpu` dropped to 11% in Transitioning territory — actionable
- Several nodes showed PreTransition on network early on — the system correctly spotted accelerating changes before they became violations
- `web-01.cpu` resolved then re-entered Transitioning multiple times — realistic oscillation pattern

### Serde JSON output

The `snapshot_json()` method produced clean, structured output suitable for pushing to a monitoring pipeline. The full `serde_json::to_string_pretty(&checker)` gave 14KB of complete state including all history — useful for archival but heavy for real-time.

---

## 5. What's Missing for Production SRE Use

### 🔴 Critical gaps for production deployment

1. **No Prometheus exporter** — No `Metrics` trait impl, no histogram/histogram_vec, no OpenTelemetry integration. As-is, you'd need to bolt on your own exporter that reads `violations()` and `phase()` periodically.

2. **No async support** — The API is entirely synchronous. For a real cluster monitor you'd want `async fn` methods or at least a `Send + Sync` marker guarantee (it does implement `Send`/`Sync` via standard compiler auto-derivation on `HashMap<String, QuantityState>`, which is good).

3. **No alerting** — Zero alerting logic. No webhooks, no callback when a violation is detected, no configurable thresholds per quantity. You'd have to poll `violations()` yourself.

4. **No wall-clock timestamps** — Snapshots don't record when they were taken. `drift_rate` is "per snapshot" not "per second/minute". For real monitoring you need actual time-series with timestamps.

5. **No multi-node aggregation** — The checker is a single in-memory struct. There's no way to aggregate across distributed checkers, no remote write, no federation.

### 🟡 Medium gaps

6. **No upper-bound conservation** — Only one-sided (must not decrease). You can invert quantities for "must not exceed", but it's hacky and error-prone in documentation.

7. **Panics on unknown quantity** — Every accessor panics if the name isn't registered. In production you'd want `Result` returns or at least `Option` fallbacks. Not great for runtime reliability.

8. **Phase algorithm is simplistic** — The comparison of "recent rate vs older rate" with a noise floor works for demo scenarios but may produce false positives with noisy real-world data. No smoothing, no moving average, no configurable window sizes.

9. **No tags/labels** — Every quantity is a flat string name. In Prometheus-world, you want labels (e.g. `node="web-01", metric="cpu"`). Today you'd encode everything into the name string, which is messy.

10. **Tolerance is hardcoded at registration** — Can't adjust tolerance at runtime. For SRE you often want to tighten or loosen constraints based on time of day or incident severity.

11. **No `Deref` or `Index` sugar** — Small ergonomic issue, but `checker["web-01.cpu"]` would be nicer than `checker.current_value("web-01.cpu")`.

### 🔵 Nice-to-haves

- README examples that show how to integrate with tokio/tokio-tasks
- A simple "watch" mode that polls and prints violations on a timer
- Byte-size memory snapshots or custom display for human-readable output
- Support for `no_std` (not realistic for monitoring but would be cool)

---

## 6. Score: ★★★☆☆ (3/5 stars)

### What earns stars

- **Zero dependencies in default build** — genuinely rare and valuable
- **Phase detection** is a real differentiator; PreTransition as an early warning concept is smart
- **Clean, well-tested API** — 53 unit tests + 6 doc tests, all passing, no warnings
- **Good documentation** — doc comments on every public method with working doc-test examples
- **Serde support** via optional feature

### What costs stars

- **Not production-ready for SRE** — The README pitches this as a production monitoring tool, but it's really a library building block. No Prometheus, no async, no alerting, no timestamps.
- **Panic-based error handling** — `unwrap_or_else(|| panic!(...))` on every accessor is fine for tests but dangerous for a 24/7 monitoring process
- **No time-series fundamentals** — Snapshots without timestamps means you can't compute meaningful rates (per-second, per-minute)
- **Phase detection is fragile** — No smoothing, no configurable window. Works for demos but may struggle in noisy production environments

### Verdict

**Good library — wrong packaging.** If this were positioned as "a simple building block for conservation invariants" I'd give it 4 stars. But it's marketed for "production monitoring" and doesn't have the infrastructure integration (Prometheus, metrics, alerting) that actual SRE teams need. 

Use it as a lightweight embedded checker in a single binary or test harness — but don't build your cluster monitoring stack on it yet.

---

## Appendix: Example Output (cluster_monitoring with serde)

The full example is in `examples/cluster_monitoring.rs`. Run with:

```bash
cargo run --features serde --example cluster_monitoring
```

The Serde JSON snapshot at the end produces output like:

```json
[{
  "conserved": false,
  "current": 11.0,
  "initial": 61.0,
  "name": "db-01.cpu",
  "tolerance": 0.0
}, {
  "conserved": false,
  "current": 16.0,
  "initial": 67.0,
  "name": "web-02.cpu",
  "tolerance": 0.0
}]
```

This is clean enough to feed into a log pipeline or push to a lightweight API, but lacks timestamps, labels, and metric type — you'd need to wrap it before feeding to Prometheus.
