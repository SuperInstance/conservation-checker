#![deny(unsafe_code)]

#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};

use std::collections::HashMap;

/// Phase detected from a quantity's rate-of-change history.
///
/// Each variant describes the trajectory of a tracked quantity relative
/// to its conservation boundary (initial value minus tolerance).
///
/// # Transitions
///
/// ```text
/// Stable → PreTransition → Transitioning → Resolving → Stable
/// ```
///
/// Use [`ConservationChecker::phase`] to obtain the current phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Phase {
    /// Rate of change is near zero — the quantity is holding steady.
    Stable,
    /// Rate of change is accelerating but hasn't crossed the tolerance threshold.
    ///
    /// This is an early warning: the quantity is still conserved, but its
    /// velocity is increasing and may lead to a violation.
    PreTransition,
    /// Value is actively decreasing beyond tolerance — the conservation law is violated.
    Transitioning,
    /// Value was decreasing but is now recovering toward the baseline.
    Resolving,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Stable => write!(f, "Stable"),
            Phase::PreTransition => write!(f, "PreTransition"),
            Phase::Transitioning => write!(f, "Transitioning"),
            Phase::Resolving => write!(f, "Resolving"),
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct QuantityState {
    initial: f64,
    current: f64,
    tolerance: f64,
    history: Vec<f64>,
}

/// Tracker for one-sided conservation laws.
///
/// A `ConservationChecker` monitors named quantities that must not decrease
/// (beyond an optional tolerance). It records snapshots over time so you can
/// detect drift, phase transitions, and violations.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConservationChecker {
    quantities: HashMap<String, QuantityState>,
}

impl Default for ConservationChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ConservationChecker {
    /// Create a new, empty tracker.
    ///
    /// # Example
    ///
    /// ```
    /// use conservation_checker::ConservationChecker;
    ///
    /// let checker = ConservationChecker::new();
    /// assert!(checker.registered().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            quantities: HashMap::new(),
        }
    }

    /// Register a named quantity with an initial value and tolerance.
    ///
    /// `tolerance` is the maximum allowed decrease from the initial value
    /// before the quantity is considered *violated*. Use `0.0` for strict
    /// conservation.
    ///
    /// # Example
    ///
    /// ```
    /// use conservation_checker::ConservationChecker;
    ///
    /// let mut checker = ConservationChecker::new();
    /// checker.register("energy", 100.0, 5.0);  // allow up to 5.0 decrease
    /// checker.register("budget", 1000.0, 0.0); // strict: any decrease violates
    ///
    /// assert!(checker.is_conserved("energy"));
    /// assert!(checker.is_conserved("budget"));
    /// ```
    pub fn register(&mut self, name: impl Into<String>, initial_value: f64, tolerance: f64) {
        let name = name.into();
        self.quantities.insert(
            name,
            QuantityState {
                initial: initial_value,
                current: initial_value,
                tolerance,
                history: vec![initial_value],
            },
        );
    }

    /// Update the current value of a registered quantity.
    ///
    /// Call this whenever the tracked quantity changes. After updating, use
    /// [`is_conserved`](Self::is_conserved) to check whether the new value
    /// is still within tolerance.
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    ///
    /// # Example
    ///
    /// ```
    /// use conservation_checker::ConservationChecker;
    ///
    /// let mut checker = ConservationChecker::new();
    /// checker.register("tokens", 100.0, 0.0);
    /// checker.update("tokens", 80.0);
    ///
    /// assert!(!checker.is_conserved("tokens"));
    /// ```
    pub fn update(&mut self, name: &str, value: f64) {
        let state = self
            .quantities
            .get_mut(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));
        state.current = value;
    }

    /// Check whether a quantity is still conserved.
    ///
    /// A quantity is conserved when `current >= initial - tolerance`.
    /// Increases are always OK (one-sided conservation).
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    pub fn is_conserved(&self, name: &str) -> bool {
        let state = self
            .quantities
            .get(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));
        state.current >= state.initial - state.tolerance
    }

    /// Return the names of all quantities that are currently violated.
    pub fn violations(&self) -> Vec<String> {
        self.quantities
            .iter()
            .filter(|(_, state)| state.current < state.initial - state.tolerance)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Record a snapshot of every quantity's current value into its history.
    ///
    /// Call this periodically (e.g. once per tick, request, or batch) to
    /// build a time-series that [`phase`](Self::phase) and
    /// [`drift_rate`](Self::drift_rate) can analyse.
    ///
    /// # Example
    ///
    /// ```
    /// use conservation_checker::ConservationChecker;
    ///
    /// let mut checker = ConservationChecker::new();
    /// checker.register("budget", 500.0, 100.0);
    ///
    /// checker.update("budget", 450.0);
    /// checker.snapshot();
    ///
    /// checker.update("budget", 420.0);
    /// checker.snapshot();
    ///
    /// // drift_rate now compares first and last snapshot
    /// let drift = checker.drift_rate("budget");
    /// assert!(drift < 0.0); // budget is drifting downward
    /// ```
    pub fn snapshot(&mut self) {
        for state in self.quantities.values_mut() {
            state.history.push(state.current);
        }
    }

    /// Detect the current phase of a quantity based on its history.
    ///
    /// Uses the most recent snapshots to compute a short-term rate of change
    /// and classifies the trajectory as [`Phase::Stable`], [`Phase::PreTransition`],
    /// [`Phase::Transitioning`], or [`Phase::Resolving`].
    ///
    /// Requires at least 3 snapshots to return anything other than `Stable`.
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    ///
    /// # Example
    ///
    /// ```
    /// use conservation_checker::{ConservationChecker, Phase};
    ///
    /// let mut checker = ConservationChecker::new();
    /// checker.register("energy", 100.0, 0.0);
    ///
    /// // Drain it down
    /// checker.update("energy", 90.0);
    /// checker.snapshot();
    /// checker.update("energy", 80.0);
    /// checker.snapshot();
    ///
    /// assert_eq!(checker.phase("energy"), Phase::Transitioning);
    /// ```
    pub fn phase(&self, name: &str) -> Phase {
        let state = self
            .quantities
            .get(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));

        let _rate = self.drift_rate(name);

        if state.history.len() < 3 {
            return Phase::Stable;
        }

        let is_violated = !self.is_conserved(name);

        let len = state.history.len();

        // Recent per-step rate (last two snapshots)
        let recent_rate = if len >= 2 {
            state.history[len - 1] - state.history[len - 2]
        } else {
            0.0
        };

        // Older per-step rate (two snapshots before the recent pair)
        let older_rate = if len >= 4 {
            state.history[len - 3] - state.history[len - 4]
        } else {
            0.0
        };

        // Use a small absolute threshold so we don't classify tiny fluctuations
        let abs_recent = recent_rate.abs();
        let noise_floor = state.tolerance.max(1.0) * 0.01;

        if is_violated && recent_rate < -noise_floor {
            Phase::Transitioning
        } else if is_violated && recent_rate > noise_floor {
            Phase::Resolving
        } else if !is_violated && abs_recent > noise_floor && abs_recent > older_rate.abs() {
            Phase::PreTransition
        } else {
            Phase::Stable
        }
    }

    /// Compute the average rate of change per snapshot for a quantity.
    ///
    /// Computed as `(last_value - first_value) / (snapshot_count - 1)`.
    /// Returns `0.0` when there are fewer than two snapshots.
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    pub fn drift_rate(&self, name: &str) -> f64 {
        let state = self
            .quantities
            .get(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));

        if state.history.len() < 2 {
            return 0.0;
        }

        let n = state.history.len() as f64;
        // Simple: (last - first) / (n - 1)
        (state.history.last().unwrap() - state.history.first().unwrap()) / (n - 1.0)
    }

    /// Get the current value of a quantity.
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    pub fn current_value(&self, name: &str) -> f64 {
        let state = self
            .quantities
            .get(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));
        state.current
    }

    /// Get the initial value the quantity was registered with.
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    pub fn initial_value(&self, name: &str) -> f64 {
        let state = self
            .quantities
            .get(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));
        state.initial
    }

    /// Get the number of snapshots recorded for a quantity (including the initial value).
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    pub fn snapshot_count(&self, name: &str) -> usize {
        let state = self
            .quantities
            .get(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));
        state.history.len()
    }

    /// List all registered quantity names in arbitrary order.
    pub fn registered(&self) -> Vec<String> {
        self.quantities.keys().cloned().collect()
    }

    /// Remove a quantity from the tracker.
    ///
    /// Returns `true` if the quantity existed and was removed, `false` otherwise.
    pub fn deregister(&mut self, name: &str) -> bool {
        self.quantities.remove(name).is_some()
    }

    /// Reset a quantity's initial value to its current value, clearing any violations.
    ///
    /// Useful after resolving a violation to establish a new baseline without
    /// re-registering the quantity.
    ///
    /// # Panics
    ///
    /// Panics if `name` has not been registered.
    ///
    /// # Example
    ///
    /// ```
    /// use conservation_checker::ConservationChecker;
    ///
    /// let mut checker = ConservationChecker::new();
    /// checker.register("budget", 100.0, 0.0);
    /// checker.update("budget", 50.0);
    ///
    /// assert!(!checker.is_conserved("budget"));
    ///
    /// checker.reset_baseline("budget");
    /// assert!(checker.is_conserved("budget"));
    /// assert_eq!(checker.initial_value("budget"), 50.0);
    /// ```
    pub fn reset_baseline(&mut self, name: &str) {
        let state = self
            .quantities
            .get_mut(name)
            .unwrap_or_else(|| panic!("quantity '{}' not registered", name));
        state.initial = state.current;
    }

    /// Serialize a snapshot of all quantities to a JSON string.
    ///
    /// Produces a JSON object with each quantity's name, current value,
    /// initial value, tolerance, and conserved status. Useful for
    /// Prometheus-style export or logging.
    ///
    /// Requires the `serde` feature.
    #[cfg(feature = "serde")]
    pub fn snapshot_json(&self) -> String {
        use serde_json::json;
        let quantities: Vec<serde_json::Value> = self
            .quantities
            .iter()
            .map(|(name, state)| {
                json!({
                    "name": name,
                    "initial": state.initial,
                    "current": state.current,
                    "tolerance": state.tolerance,
                    "conserved": state.current >= state.initial - state.tolerance,
                })
            })
            .collect();
        serde_json::to_string(&quantities).unwrap_or_else(|_| "[]".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction & registration ──────────────────────────────────

    #[test]
    fn new_tracker_is_empty() {
        let c = ConservationChecker::new();
        assert!(c.registered().is_empty());
        assert!(c.violations().is_empty());
    }

    #[test]
    fn default_equals_new() {
        let a = ConservationChecker::new();
        let b = ConservationChecker::default();
        assert_eq!(a.registered(), b.registered());
        assert_eq!(a.violations(), b.violations());
    }

    #[test]
    fn register_single_quantity() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 0.0);
        assert_eq!(c.registered(), vec!["energy"]);
    }

    #[test]
    fn register_multiple_quantities() {
        let mut c = ConservationChecker::new();
        c.register("a", 10.0, 0.0);
        c.register("b", 20.0, 0.0);
        c.register("c", 30.0, 0.0);
        let mut names = c.registered();
        names.sort();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn register_overwrites_existing() {
        let mut c = ConservationChecker::new();
        c.register("x", 50.0, 1.0);
        c.register("x", 99.0, 2.0);
        assert_eq!(c.current_value("x"), 99.0);
        assert!((c.initial_value("x") - 99.0).abs() < f64::EPSILON);
    }

    #[test]
    fn initial_and_current_match_after_register() {
        let mut c = ConservationChecker::new();
        c.register("tokens", 42.0, 0.0);
        assert!((c.initial_value("tokens") - 42.0).abs() < f64::EPSILON);
        assert!((c.current_value("tokens") - 42.0).abs() < f64::EPSILON);
    }

    // ── Updates ──────────────────────────────────────────────────────

    #[test]
    fn update_changes_current_value() {
        let mut c = ConservationChecker::new();
        c.register("budget", 1000.0, 0.0);
        c.update("budget", 950.0);
        assert!((c.current_value("budget") - 950.0).abs() < f64::EPSILON);
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn update_panics_on_unknown_name() {
        let mut c = ConservationChecker::new();
        c.update("ghost", 10.0);
    }

    #[test]
    fn update_to_same_value_is_fine() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        c.update("q", 10.0);
        assert!(c.is_conserved("q"));
    }

    #[test]
    fn update_to_higher_value_is_fine() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        c.update("q", 999.0);
        assert!(c.is_conserved("q"));
    }

    // ── Conservation checks ──────────────────────────────────────────

    #[test]
    fn is_conserved_no_change() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 0.0);
        assert!(c.is_conserved("energy"));
    }

    #[test]
    fn is_conserved_increase_ok() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 0.0);
        c.update("energy", 150.0);
        assert!(c.is_conserved("energy"));
    }

    #[test]
    fn is_conserved_decrease_violates_strict() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 0.0);
        c.update("energy", 99.9);
        assert!(!c.is_conserved("energy"));
    }

    #[test]
    fn is_conserved_within_tolerance() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 5.0);
        c.update("energy", 96.0);
        assert!(c.is_conserved("energy"));
    }

    #[test]
    fn is_conserved_exactly_at_tolerance_boundary() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 5.0);
        c.update("energy", 95.0); // initial - tolerance = 95
        assert!(c.is_conserved("energy"));
    }

    #[test]
    fn is_conserved_just_past_tolerance() {
        let mut c = ConservationChecker::new();
        c.register("energy", 100.0, 5.0);
        c.update("energy", 94.999);
        assert!(!c.is_conserved("energy"));
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn is_conserved_panics_on_unknown() {
        ConservationChecker::new().is_conserved("nope");
    }

    // ── Violations ───────────────────────────────────────────────────

    #[test]
    fn violations_none_when_all_ok() {
        let mut c = ConservationChecker::new();
        c.register("a", 10.0, 0.0);
        c.register("b", 20.0, 0.0);
        assert!(c.violations().is_empty());
    }

    #[test]
    fn violations_reports_decreased() {
        let mut c = ConservationChecker::new();
        c.register("a", 10.0, 0.0);
        c.register("b", 20.0, 0.0);
        c.update("a", 5.0);
        let v = c.violations();
        assert_eq!(v, vec!["a"]);
    }

    #[test]
    fn violations_multiple() {
        let mut c = ConservationChecker::new();
        c.register("a", 10.0, 0.0);
        c.register("b", 20.0, 0.0);
        c.register("c", 30.0, 0.0);
        c.update("a", 5.0);
        c.update("c", 25.0);
        let mut v = c.violations();
        v.sort();
        assert_eq!(v, vec!["a", "c"]);
    }

    // ── Snapshots & history ──────────────────────────────────────────

    #[test]
    fn snapshot_increments_count() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        assert_eq!(c.snapshot_count("q"), 1); // initial counts
        c.snapshot();
        assert_eq!(c.snapshot_count("q"), 2);
        c.snapshot();
        assert_eq!(c.snapshot_count("q"), 3);
    }

    #[test]
    fn snapshot_records_current_values() {
        let mut c = ConservationChecker::new();
        c.register("x", 100.0, 0.0);
        c.update("x", 90.0);
        c.snapshot();
        // drift_rate should now be (90 - 100) / 1 = -10
        assert!((c.drift_rate("x") - (-10.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn snapshot_captures_all_quantities() {
        let mut c = ConservationChecker::new();
        c.register("a", 10.0, 0.0);
        c.register("b", 20.0, 0.0);
        c.snapshot();
        assert_eq!(c.snapshot_count("a"), 2);
        assert_eq!(c.snapshot_count("b"), 2);
    }

    // ── Drift rate ───────────────────────────────────────────────────

    #[test]
    fn drift_rate_zero_with_one_snapshot() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        assert!((c.drift_rate("q") - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn drift_rate_positive_on_increase() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        c.update("q", 20.0);
        c.snapshot();
        // (20 - 10) / 1 = 10
        assert!((c.drift_rate("q") - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn drift_rate_negative_on_decrease() {
        let mut c = ConservationChecker::new();
        c.register("q", 100.0, 0.0);
        c.update("q", 90.0);
        c.snapshot();
        // (90 - 100) / 1 = -10
        assert!((c.drift_rate("q") - (-10.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn drift_rate_averages_over_many_snapshots() {
        let mut c = ConservationChecker::new();
        c.register("q", 0.0, 0.0);
        // 0 -> 10 -> 20 -> 30  (3 snapshots after initial)
        for v in [10.0, 20.0, 30.0] {
            c.update("q", v);
            c.snapshot();
        }
        // (30 - 0) / (4-1) = 10
        assert!((c.drift_rate("q") - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn drift_rate_panics_on_unknown() {
        ConservationChecker::new().drift_rate("nope");
    }

    // ── Phase detection ──────────────────────────────────────────────

    #[test]
    fn phase_stable_when_no_change() {
        let mut c = ConservationChecker::new();
        c.register("q", 100.0, 0.0);
        c.snapshot();
        c.snapshot();
        assert_eq!(c.phase("q"), Phase::Stable);
    }

    #[test]
    fn phase_transitioning_when_decreasing_and_violated() {
        let mut c = ConservationChecker::new();
        c.register("q", 100.0, 0.0);
        c.update("q", 90.0);
        c.snapshot();
        c.update("q", 80.0);
        c.snapshot();
        assert_eq!(c.phase("q"), Phase::Transitioning);
    }

    #[test]
    fn phase_resolving_when_violated_but_recovering() {
        let mut c = ConservationChecker::new();
        c.register("q", 100.0, 5.0);
        // Drop below tolerance
        c.update("q", 90.0);
        c.snapshot();
        c.update("q", 80.0);
        c.snapshot();
        // Now increase — still violated (92 < 95) but recovering
        c.update("q", 92.0);
        c.snapshot();
        assert_eq!(c.phase("q"), Phase::Resolving);
    }

    #[test]
    fn phase_pre_transition_when_accelerating() {
        let mut c = ConservationChecker::new();
        c.register("q", 100.0, 50.0); // large tolerance
        c.update("q", 99.0);
        c.snapshot();
        c.update("q", 96.0); // accelerating downward (-3 vs -1)
        c.snapshot();
        // Accelerating downward but not yet violated (96 > 50)
        assert_eq!(c.phase("q"), Phase::PreTransition);
    }

    #[test]
    fn phase_stable_with_few_snapshots() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        // Only 1 history entry (the initial)
        assert_eq!(c.phase("q"), Phase::Stable);
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn phase_panics_on_unknown() {
        ConservationChecker::new().phase("nope");
    }

    // ── Phase display ────────────────────────────────────────────────

    #[test]
    fn phase_display() {
        assert_eq!(format!("{}", Phase::Stable), "Stable");
        assert_eq!(format!("{}", Phase::PreTransition), "PreTransition");
        assert_eq!(format!("{}", Phase::Transitioning), "Transitioning");
        assert_eq!(format!("{}", Phase::Resolving), "Resolving");
    }

    // ── Deregister ───────────────────────────────────────────────────

    #[test]
    fn deregister_removes_quantity() {
        let mut c = ConservationChecker::new();
        c.register("x", 10.0, 0.0);
        assert!(c.deregister("x"));
        assert!(c.registered().is_empty());
    }

    #[test]
    fn deregister_unknown_returns_false() {
        let mut c = ConservationChecker::new();
        assert!(!c.deregister("ghost"));
    }

    // ── Reset baseline ───────────────────────────────────────────────

    #[test]
    fn reset_baseline_clears_violation() {
        let mut c = ConservationChecker::new();
        c.register("budget", 100.0, 0.0);
        c.update("budget", 50.0);
        assert!(!c.is_conserved("budget"));
        c.reset_baseline("budget");
        assert!(c.is_conserved("budget"));
        assert!((c.initial_value("budget") - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn reset_baseline_panics_on_unknown() {
        ConservationChecker::new().reset_baseline("nope");
    }

    // ── Snapshot count ───────────────────────────────────────────────

    #[test]
    #[should_panic(expected = "not registered")]
    fn snapshot_count_panics_on_unknown() {
        ConservationChecker::new().snapshot_count("nope");
    }

    // ── Current / initial value ──────────────────────────────────────

    #[test]
    #[should_panic(expected = "not registered")]
    fn current_value_panics_on_unknown() {
        ConservationChecker::new().current_value("nope");
    }

    #[test]
    #[should_panic(expected = "not registered")]
    fn initial_value_panics_on_unknown() {
        ConservationChecker::new().initial_value("nope");
    }

    // ── Integration-style tests ──────────────────────────────────────

    #[test]
    fn budget_tracking_scenario() {
        let mut c = ConservationChecker::new();
        c.register("remaining", 5000.0, 1000.0);

        c.update("remaining", 4500.0);
        c.snapshot();
        assert!(c.is_conserved("remaining"));

        c.update("remaining", 4200.0);
        c.snapshot();
        assert!(c.is_conserved("remaining"));

        c.update("remaining", 3900.0);
        c.snapshot();
        assert!(!c.is_conserved("remaining"));
    }

    #[test]
    fn token_budget_depletion() {
        let mut c = ConservationChecker::new();
        c.register("tokens", 1000.0, 0.0);

        for _ in 0..5 {
            c.update("tokens", c.current_value("tokens") - 100.0);
            c.snapshot();
        }
        assert!(!c.is_conserved("tokens"));
        let v = c.violations();
        assert!(v.contains(&"tokens".to_string()));
    }

    #[test]
    fn multiple_snapshots_drift_calculation() {
        let mut c = ConservationChecker::new();
        c.register("q", 0.0, 0.0);
        for i in 1..=10 {
            c.update("q", i as f64 * 5.0);
            c.snapshot();
        }
        // first=0, last=50, n=11 entries
        assert!((c.drift_rate("q") - (50.0 / 10.0)).abs() < 1e-9);
    }

    #[test]
    fn tolerance_zero_strict() {
        let mut c = ConservationChecker::new();
        c.register("strict", 100.0, 0.0);
        c.update("strict", 99.999);
        assert!(!c.is_conserved("strict"));
    }

    #[test]
    fn large_tolerance_never_violates() {
        let mut c = ConservationChecker::new();
        c.register("lenient", 100.0, 10000.0);
        c.update("lenient", -9000.0);
        assert!(c.is_conserved("lenient"));
    }

    #[test]
    fn negative_values_work() {
        let mut c = ConservationChecker::new();
        c.register("temp", -40.0, 0.0);
        c.update("temp", -50.0);
        assert!(!c.is_conserved("temp"));
        c.update("temp", -30.0);
        assert!(c.is_conserved("temp"));
    }

    #[test]
    fn clone_independence() {
        let mut c = ConservationChecker::new();
        c.register("q", 10.0, 0.0);
        let mut c2 = c.clone();
        c2.update("q", 5.0);
        assert!(c.is_conserved("q"));
        assert!(!c2.is_conserved("q"));
    }

    #[test]
    fn snapshot_count_panics_on_unknown2() {
        let c = ConservationChecker::new();
        let result = std::panic::catch_unwind(|| c.snapshot_count("nope"));
        assert!(result.is_err());
    }

    #[test]
    fn register_with_string_ref() {
        let mut c = ConservationChecker::new();
        let name = "quantity";
        c.register(name, 42.0, 1.0);
        assert!(c.is_conserved("quantity"));
    }

    #[test]
    fn empty_violations_after_register() {
        let mut c = ConservationChecker::new();
        c.register("a", 10.0, 0.0);
        assert!(c.violations().is_empty());
    }

    #[test]
    fn phase_stable_on_increase() {
        let mut c = ConservationChecker::new();
        c.register("q", 100.0, 10.0);
        // Constant rate of increase across 4+ snapshots
        c.update("q", 105.0);
        c.snapshot();
        c.update("q", 110.0);
        c.snapshot();
        c.update("q", 115.0); // same +5 rate
        c.snapshot();
        assert_eq!(c.phase("q"), Phase::Stable);
    }
}
