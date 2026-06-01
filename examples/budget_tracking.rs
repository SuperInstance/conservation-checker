use conservation_checker::{ConservationChecker, Phase};

fn main() {
    let mut budget = ConservationChecker::new();

    // Monthly budget: $5000, we allow going $100 over as a buffer
    budget.register("monthly_budget", 5000.0, 100.0);

    let expenses = [1200.0, 800.0, 1500.0, 900.0, 700.0, 350.0];

    for (i, expense) in expenses.iter().enumerate() {
        let remaining = budget.current_value("monthly_budget") - expense;
        budget.update("monthly_budget", remaining);
        budget.snapshot();

        let phase = budget.phase("monthly_budget");
        let drift = budget.drift_rate("monthly_budget");

        println!(
            "Day {}: spent ${:.0}, remaining ${:.0} | phase={:?}, drift={:.1}/day",
            i + 1,
            expense,
            remaining,
            phase,
            drift,
        );

        if phase == Phase::Transitioning {
            println!("  ⚠️  Budget is depleting fast!");
        }
    }

    if !budget.is_conserved("monthly_budget") {
        println!("\n❌ Budget violated! Over by ${:.0}", 
                 budget.initial_value("monthly_budget") - budget.current_value("monthly_budget"));
        println!("   Violations: {:?}", budget.violations());
    } else {
        println!("\n✅ Budget intact with ${:.0} remaining", budget.current_value("monthly_budget"));
    }
}
