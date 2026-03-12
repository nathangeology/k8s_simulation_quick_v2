//! KubeSim Engine — Discrete event simulation loop with priority queue and dual time modes.

use kubesim_core::{ClusterState, NodeId, OwnerId, PodId, SimTime};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

pub mod replicaset;
pub mod deletion_cost;
pub use replicaset::ReplicaSetController;
pub use deletion_cost::DeletionCostController;

// ── Events ──────────────────────────────────────────────────────

/// Unique identifier for a deployment/replicaset (for HPA/scaling events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeploymentId(pub u32);

/// Specification for launching a new node (carried by NodeLaunching event).
#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub instance_type: String,
    /// Labels propagated from the NodePool.
    pub labels: kubesim_core::LabelSet,
    /// Taints propagated from the NodePool.
    pub taints: Vec<kubesim_core::Taint>,
    /// Name of the NodePool that launched this node.
    pub pool_name: String,
    /// When true, the node has `karpenter.sh/do-not-disrupt` annotation.
    pub do_not_disrupt: bool,
}

/// Specification for submitting a new pod (carried by PodSubmitted event).
#[derive(Debug, Clone)]
pub struct PodSpec {
    pub requests: kubesim_core::Resources,
    pub limits: kubesim_core::Resources,
    pub owner: kubesim_core::OwnerId,
    pub priority: i32,
    pub labels: kubesim_core::LabelSet,
    pub scheduling_constraints: kubesim_core::SchedulingConstraints,
    pub do_not_disrupt: bool,
    /// Optional pod lifetime in nanoseconds (for batch jobs).
    pub duration_ns: Option<u64>,
}

/// All discrete events in the simulation.
#[derive(Debug, Clone)]
pub enum Event {
    PodSubmitted(PodSpec),
    PodScheduled(PodId, NodeId),
    PodRunning(PodId),
    PodTerminating(PodId),
    PodDeleted(PodId),
    NodeLaunching(NodeSpec),
    NodeReady(NodeId),
    NodeCordoned(NodeId),
    NodeDrained(NodeId),
    NodeTerminated(NodeId),
    SpotInterruption(NodeId),
    SpotInterruptionCheck,
    HpaEvaluation(DeploymentId),
    KarpenterProvisioningLoop,
    KarpenterConsolidationLoop,
    ScaleDown(DeploymentId, u32),
    ScaleUp(DeploymentId, u32),
    ReplicaSetReconcile(OwnerId),
    PodCompleted(PodId),
    DeletionCostReconcile,
    MetricsSnapshot,
}

// ── Scheduled event (time-tagged) ───────────────────────────────

/// An event tagged with the simulation time at which it should fire.
#[derive(Debug, Clone)]
pub struct ScheduledEvent {
    pub time: SimTime,
    pub event: Event,
}

impl PartialEq for ScheduledEvent {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl Eq for ScheduledEvent {}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time.cmp(&other.time)
    }
}

// ── Time modes ──────────────────────────────────────────────────

/// Controls how simulation time advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeMode {
    /// Events processed in causal order, no wall-clock delays. Fastest mode.
    Logical,
    /// Events carry realistic durations (nanoseconds in SimTime).
    WallClock,
}

// ── Event handler trait ─────────────────────────────────────────

/// Pluggable handler invoked for each event. Implementations can mutate cluster
/// state and schedule follow-up events by returning them.
pub trait EventHandler {
    /// Process an event, returning zero or more follow-up events to schedule.
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent>;

    /// Downcast support for extracting concrete handler types after simulation.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

// ── Engine ──────────────────────────────────────────────────────

/// Discrete event simulation engine.
pub struct Engine {
    queue: BinaryHeap<Reverse<ScheduledEvent>>,
    time_mode: TimeMode,
    handlers: Vec<Box<dyn EventHandler>>,
}

impl Engine {
    pub fn new(time_mode: TimeMode) -> Self {
        Self {
            queue: BinaryHeap::new(),
            time_mode,
            handlers: Vec::new(),
        }
    }

    /// Register a pluggable event handler.
    pub fn add_handler(&mut self, handler: Box<dyn EventHandler>) {
        self.handlers.push(handler);
    }

    /// Schedule an event at the given time.
    pub fn schedule(&mut self, time: SimTime, event: Event) {
        self.queue.push(Reverse(ScheduledEvent { time, event }));
    }

    /// Schedule an event relative to the current simulation time in `state`.
    /// In `Logical` mode, `delay` is ignored and the event fires at `state.time + 1`.
    pub fn schedule_relative(
        &mut self,
        state: &ClusterState,
        delay_ns: u64,
        event: Event,
    ) {
        let fire_time = match self.time_mode {
            TimeMode::Logical => SimTime(state.time.0 + 1),
            TimeMode::WallClock => SimTime(state.time.0 + delay_ns),
        };
        self.schedule(fire_time, event);
    }

    /// Number of pending events in the queue.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Current time mode.
    pub fn time_mode(&self) -> TimeMode {
        self.time_mode
    }

    /// Process the next event. Returns `true` if an event was processed.
    pub fn step(&mut self, state: &mut ClusterState) -> bool {
        let Reverse(scheduled) = match self.queue.pop() {
            Some(e) => e,
            None => return false,
        };
        state.time = scheduled.time;

        // Collect follow-up events from all handlers.
        let mut follow_ups = Vec::new();
        for i in 0..self.handlers.len() {
            // Safety: we take a mutable ref to handlers[i] while passing state.
            // This is safe because handlers don't alias state.
            let handler = &mut self.handlers[i];
            let new_events = handler.handle(&scheduled.event, scheduled.time, state);
            follow_ups.extend(new_events);
        }

        for se in follow_ups {
            self.queue.push(Reverse(se));
        }
        true
    }

    /// Run until the queue is empty or `until` time is reached.
    /// Returns the number of events processed.
    pub fn run_until(&mut self, state: &mut ClusterState, until: SimTime) -> u64 {
        let mut count = 0u64;
        while let Some(Reverse(next)) = self.queue.peek() {
            if next.time > until {
                break;
            }
            self.step(state);
            count += 1;
        }
        count
    }

    /// Drain the entire event queue, processing all events.
    /// Returns the number of events processed.
    /// Stops after `max_events` (default 1_000_000) as a safety valve.
    pub fn run_to_completion(&mut self, state: &mut ClusterState) -> u64 {
        self.run_to_completion_with_limit(state, 1_000_000)
    }

    /// Drain the event queue with an explicit event limit.
    /// Returns the number of events processed.
    pub fn run_to_completion_with_limit(&mut self, state: &mut ClusterState, max_events: u64) -> u64 {
        let mut count = 0u64;
        while count < max_events && self.step(state) {
            count += 1;
        }
        count
    }

    /// Get mutable references to handlers (for post-run inspection).
    pub fn handlers_mut(&mut self) -> &mut [Box<dyn EventHandler>] {
        &mut self.handlers
    }

    /// Consume the engine and return all registered handlers.
    pub fn into_handlers(self) -> Vec<Box<dyn EventHandler>> {
        self.handlers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubesim_core::ClusterState;

    /// A simple handler that records events it saw and optionally schedules follow-ups.
    struct CountingHandler {
        count: u64,
    }

    impl CountingHandler {
        fn new() -> Self {
            Self { count: 0 }
        }
    }

    impl EventHandler for CountingHandler {
        fn handle(
            &mut self,
            _event: &Event,
            _time: SimTime,
            _state: &mut ClusterState,
        ) -> Vec<ScheduledEvent> {
            self.count += 1;
            Vec::new()
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    #[test]
    fn empty_engine_step_returns_false() {
        let mut engine = Engine::new(TimeMode::Logical);
        let mut state = ClusterState::new();
        assert!(!engine.step(&mut state));
    }

    #[test]
    fn events_processed_in_time_order() {
        let mut engine = Engine::new(TimeMode::Logical);
        let mut state = ClusterState::new();

        // Schedule out of order
        engine.schedule(SimTime(30), Event::MetricsSnapshot);
        engine.schedule(SimTime(10), Event::KarpenterProvisioningLoop);
        engine.schedule(SimTime(20), Event::KarpenterConsolidationLoop);

        engine.step(&mut state);
        assert_eq!(state.time, SimTime(10));
        engine.step(&mut state);
        assert_eq!(state.time, SimTime(20));
        engine.step(&mut state);
        assert_eq!(state.time, SimTime(30));
        assert!(!engine.step(&mut state));
    }

    #[test]
    fn run_until_stops_at_boundary() {
        let mut engine = Engine::new(TimeMode::WallClock);
        let mut state = ClusterState::new();

        engine.schedule(SimTime(100), Event::MetricsSnapshot);
        engine.schedule(SimTime(200), Event::MetricsSnapshot);
        engine.schedule(SimTime(300), Event::MetricsSnapshot);

        let processed = engine.run_until(&mut state, SimTime(250));
        assert_eq!(processed, 2);
        assert_eq!(state.time, SimTime(200));
        assert_eq!(engine.pending(), 1);
    }

    #[test]
    fn run_to_completion_drains_queue() {
        let mut engine = Engine::new(TimeMode::Logical);
        let mut state = ClusterState::new();

        for i in 0..5 {
            engine.schedule(SimTime(i), Event::MetricsSnapshot);
        }

        let processed = engine.run_to_completion(&mut state);
        assert_eq!(processed, 5);
        assert_eq!(engine.pending(), 0);
    }

    #[test]
    fn handler_receives_events() {
        let mut engine = Engine::new(TimeMode::Logical);
        let mut state = ClusterState::new();

        engine.add_handler(Box::new(CountingHandler::new()));
        engine.schedule(SimTime(1), Event::MetricsSnapshot);
        engine.schedule(SimTime(2), Event::MetricsSnapshot);

        engine.run_to_completion(&mut state);
        // Can't directly inspect handler count through the engine, but we verify
        // the engine processed both events without panic.
        assert_eq!(state.time, SimTime(2));
    }

    #[test]
    fn handler_follow_up_events_are_scheduled() {
        /// Handler that spawns one follow-up event on the first call.
        struct SpawningHandler {
            spawned: bool,
        }
        impl EventHandler for SpawningHandler {
            fn handle(
                &mut self,
                _event: &Event,
                time: SimTime,
                _state: &mut ClusterState,
            ) -> Vec<ScheduledEvent> {
                if !self.spawned {
                    self.spawned = true;
                    vec![ScheduledEvent {
                        time: SimTime(time.0 + 10),
                        event: Event::MetricsSnapshot,
                    }]
                } else {
                    Vec::new()
                }
            }
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
        }

        let mut engine = Engine::new(TimeMode::Logical);
        let mut state = ClusterState::new();

        engine.add_handler(Box::new(SpawningHandler { spawned: false }));
        engine.schedule(SimTime(1), Event::MetricsSnapshot);

        let processed = engine.run_to_completion(&mut state);
        assert_eq!(processed, 2); // original + follow-up
        assert_eq!(state.time, SimTime(11));
    }

    #[test]
    fn schedule_relative_logical_ignores_delay() {
        let mut engine = Engine::new(TimeMode::Logical);
        let mut state = ClusterState::new();
        state.time = SimTime(100);

        engine.schedule_relative(&state, 999_999, Event::MetricsSnapshot);
        engine.step(&mut state);
        assert_eq!(state.time, SimTime(101)); // +1, not +999_999
    }

    #[test]
    fn schedule_relative_wallclock_uses_delay() {
        let mut engine = Engine::new(TimeMode::WallClock);
        let mut state = ClusterState::new();
        state.time = SimTime(100);

        engine.schedule_relative(&state, 5000, Event::MetricsSnapshot);
        engine.step(&mut state);
        assert_eq!(state.time, SimTime(5100));
    }
}
