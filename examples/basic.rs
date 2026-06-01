use conservation_checker::ConservationChecker;

fn main() {
    let mut checker = ConservationChecker::new();
    checker.register("energy", 100.0, 5.0);

    println!("Initial: energy = {}", checker.current_value("energy"));

    checker.update("energy", 98.0);
    checker.snapshot();
    println!("After use: energy = {} (conserved: {})", 
             checker.current_value("energy"),
             checker.is_conserved("energy"));

    checker.update("energy", 90.0);
    checker.snapshot();
    println!("After heavy use: energy = {} (conserved: {})",
             checker.current_value("energy"),
             checker.is_conserved("energy"));
    println!("Violations: {:?}", checker.violations());
    println!("Phase: {}", checker.phase("energy"));
    println!("Drift rate: {:.2}/snapshot", checker.drift_rate("energy"));
}
