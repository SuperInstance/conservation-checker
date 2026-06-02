use conservation_checker::{ConservationChecker, Phase};
use std::collections::HashMap;

/// Simulated cluster node state
struct Node {
    name: String,
    cpu_percent: f64,
    memory_gb: f64,
    network_mbps: f64,
}

/// Simulate one monitoring tick — each node's resources change slightly
fn tick(nodes: &mut [Node], tick_id: u32) {
    // Use deterministic-ish changes so results are reproducible
    for (i, node) in nodes.iter_mut().enumerate() {
        let seed = (tick_id as usize) * 37 + i * 13;

        // CPU: varies ±5% per tick, with occasional heavy loads
        if seed % 7 == 0 && tick_id > 0 {
            node.cpu_percent = (node.cpu_percent - 15.0).max(0.0); // heavy load event
        } else {
            let delta = ((seed % 11) as f64) - 5.0; // -5 to +5
            node.cpu_percent = (node.cpu_percent + delta).clamp(0.0, 100.0);
        }

        // Memory: creeps up slowly (memory leak detection)
        node.memory_gb = (node.memory_gb + ((seed % 3) as f64) * 0.1).min(64.0);

        // Network: bursty
        if seed % 13 == 0 {
            node.network_mbps = 500.0 + (seed % 200) as f64; // spike
        } else {
            node.network_mbps = (node.network_mbps + ((seed % 7) as f64) - 3.0).max(0.0);
        }
    }
}

fn main() {
    // ── Setup: 10-node cluster ──────────────────────────────────────
    let node_names = [
        "web-01", "web-02", "web-03",
        "db-01", "db-02",
        "worker-01", "worker-02", "worker-03",
        "cache-01", "monitor-01",
    ];

    let mut nodes: Vec<Node> = node_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let seed = i * 7;
            Node {
                name: name.to_string(),
                cpu_percent: (60.0 + (seed % 20) as f64).min(100.0),
                memory_gb: 16.0 + (seed % 8) as f64,
                network_mbps: 100.0 + (seed % 50) as f64,
            }
        })
        .collect();

    // ── Conservation laws ────────────────────────────────────────────
    // CPU must not drop below 30% on any node
    // Memory must not exceed 48 GB (reverse conservation — this crate
    // only checks "must not decrease", so we invert: track "memory_remaining")
    // Network throughput must not drop below 10 Mbps
    let mut cluster = ConservationChecker::new();

    for node in &nodes {
        cluster.register(format!("{}.cpu", node.name), node.cpu_percent, 0.0);
        // Invert memory: track remaining = 64 - used, so low remaining = violation
        let mem_remaining = 64.0 - node.memory_gb;
        cluster.register(format!("{}.mem_remaining", node.name), mem_remaining, 0.0);
        cluster.register(format!("{}.net", node.name), node.network_mbps, 10.0);
    }

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║  SRE Cluster Monitoring Simulation (10 nodes, 20 ticks) ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!();

    // ── Static CPU minimum threshold ────────────────────────────────
    const CPU_MIN: f64 = 30.0;

    // ── Run simulation ──────────────────────────────────────────────
    for tick_id in 0..20 {
        tick(&mut nodes, tick_id);

        // Update all conservation values
        for node in &nodes {
            cluster.update(&format!("{}.cpu", node.name), node.cpu_percent);
            let mem_remaining = 64.0 - node.memory_gb;
            cluster.update(&format!("{}.mem_remaining", node.name), mem_remaining);
            cluster.update(&format!("{}.net", node.name), node.network_mbps);
        }

        cluster.snapshot(); // Take a snapshot every tick

        // Collect violations for this tick
        let violations = cluster.violations();

        // Collect additional SRE-relevant stats
        let mut cpu_alerts = Vec::new();
        let mut mem_alerts = Vec::new();
        let mut net_alerts = Vec::new();
        let mut phases_report = Vec::new();

        for node in &nodes {
            let cpu_ok = cluster.is_conserved(&format!("{}.cpu", node.name));
            let cpu_pct = cluster.current_value(&format!("{}.cpu", node.name));
            if cpu_pct < CPU_MIN {
                cpu_alerts.push(format!("{}@{}%", node.name, cpu_pct));
            }

            let mem_ok = cluster.is_conserved(&format!("{}.mem_remaining", node.name));
            if !mem_ok {
                let used = 64.0 - cluster.current_value(&format!("{}.mem_remaining", node.name));
                mem_alerts.push(format!("{}@{:.1}GB", node.name, used));
            }

            let net_ok = cluster.is_conserved(&format!("{}.net", node.name));
            if !net_ok {
                net_alerts.push(format!(
                    "{}@{}Mbps",
                    node.name,
                    cluster.current_value(&format!("{}.net", node.name))
                ));
            }

            // Track phases for nodes in transition
            let cpu_phase = cluster.phase(&format!("{}.cpu", node.name));
            let mem_phase = cluster.phase(&format!("{}.mem_remaining", node.name));
            let net_phase = cluster.phase(&format!("{}.net", node.name));

            if cpu_phase != Phase::Stable || mem_phase != Phase::Stable || net_phase != Phase::Stable {
                phases_report.push(format!(
                    "{}: cpu={}, mem={}, net={}",
                    node.name, cpu_phase, mem_phase, net_phase
                ));
            }
        }

        // Print tick summary
        if tick_id % 5 == 0 || !violations.is_empty() || !cpu_alerts.is_empty() || !mem_alerts.is_empty() {
            println!("── Tick {:>2} ─────────────────────", tick_id);
            println!("  Registered quantities: {}", cluster.registered().len());
            println!("  Total violations: {}", violations.len());

            if !cpu_alerts.is_empty() {
                println!("  🔴 CPU low: {}", cpu_alerts.join(", "));
            }
            if !mem_alerts.is_empty() {
                println!("  🟠 Memory high: {}", mem_alerts.join(", "));
            }
            if !net_alerts.is_empty() {
                println!("  🟡 Network low: {}", net_alerts.join(", "));
            }
            if !phases_report.is_empty() {
                for line in &phases_report {
                    println!("  📊 {}", line);
                }
            }
            println!();
        }
    }

    // ── Final summary ────────────────────────────────────────────────
    println!("═══════════════════════════════════════════════════════════════");
    println!("  🏁 FINAL STATE");
    println!("═══════════════════════════════════════════════════════════════");

    let final_violations = cluster.violations();
    println!("  Total violations at end: {}", final_violations.len());
    for v in &final_violations {
        println!("    ❌ {}", v);
    }

    // Phase distribution at end
    println!();
    println!("  Quantity phases at tick 20:");
    for name in cluster.registered() {
        let phase = cluster.phase(&name);
        let current = cluster.current_value(&name);
        let conserved = cluster.is_conserved(&name);
        let drift = cluster.drift_rate(&name);
        let snaps = cluster.snapshot_count(&name);
        println!(
            "    {} | value={:.1}, conserved={}, phase={}, drift={:+.2}/tick, {} snapshots",
            name, current, conserved, phase, drift, snaps
        );
    }

    // ── Serde snapshot ───────────────────────────────────────────────
    #[cfg(feature = "serde")]
    {
        println!();
        println!("  ┌─ Serde JSON snapshot ──────────────────────────┐");
        let json = cluster.snapshot_json();
        println!("  {}", json.replace('\n', "\n  "));
        println!("  └────────────────────────────────────────────────┘");

        // Demonstrate deserializing the whole ConservationChecker
        let json_full = serde_json::to_string_pretty(&cluster).unwrap();
        println!();
        println!("  Full state (all history) available for archival:");
        println!("  {} bytes", json_full.len());
    }
}
