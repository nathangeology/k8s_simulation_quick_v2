//! ReplicaSet controller — manages pod lifecycle and deletion-cost ordering.
//!
//! On reconcile: compares actual vs desired replica count.
//! Scale-up: submits new pods from template.
//! Scale-down: selects pods to delete ordered by:
//!   (a) pod-deletion-cost ASC
//!   (b) fewer co-located replicas on same node
//!   (c) newer pods first (higher PodId index = newer)

use kubesim_core::{
    ClusterState, DeletionCostStrategy, OwnerId, Pod, PodPhase, QoSClass, ReplicaSetId,
};

use crate::{Event, EventHandler, ScheduledEvent, SimTime};

/// ReplicaSetController handles ReplicaSetReconcile, PodTerminating, and PodDeleted events.
pub struct ReplicaSetController;

impl EventHandler for ReplicaSetController {
    fn handle(
        &mut self,
        event: &Event,
        time: SimTime,
        state: &mut ClusterState,
    ) -> Vec<ScheduledEvent> {
        match event {
            Event::ReplicaSetReconcile(owner_id) => reconcile(*owner_id, state),
            Event::PodTerminating(pod_id) | Event::PodDeleted(pod_id) => {
                if let Some(pod) = state.pods.get(*pod_id) {
                    let owner = pod.owner;
                    if find_rs(state, owner).is_some() {
                        return vec![ScheduledEvent {
                            time: SimTime(time.0 + 1),
                            event: Event::ReplicaSetReconcile(owner),
                        }];
                    }
                }
                Vec::new()
            }
            Event::ScaleUp(dep_id, count) => {
                let owner = OwnerId(dep_id.0);
                if let Some(rs_id) = find_rs(state, owner) {
                    let rs = state.replica_sets.get_mut(rs_id).unwrap();
                    rs.desired_replicas = rs.desired_replicas.saturating_add(*count);
                    return vec![ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: Event::ReplicaSetReconcile(owner),
                    }];
                }
                Vec::new()
            }
            Event::ScaleDown(dep_id, count) => {
                let owner = OwnerId(dep_id.0);
                if let Some(rs_id) = find_rs(state, owner) {
                    let rs = state.replica_sets.get_mut(rs_id).unwrap();
                    rs.desired_replicas = rs.desired_replicas.saturating_sub(*count);
                    return vec![ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: Event::ReplicaSetReconcile(owner),
                    }];
                }
                Vec::new()
            }
            Event::HpaEvaluation(deployment_id) => {
                let owner = OwnerId(deployment_id.0);
                if find_rs(state, owner).is_some() {
                    return vec![ScheduledEvent {
                        time: SimTime(time.0 + 1),
                        event: Event::ReplicaSetReconcile(owner),
                    }];
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }
}

/// Find the ReplicaSet with the given owner_id.
fn find_rs(state: &ClusterState, owner: OwnerId) -> Option<ReplicaSetId> {
    state
        .replica_sets
        .iter()
        .find(|(_, rs)| rs.owner_id == owner)
        .map(|(id, _)| id)
}

fn reconcile(owner_id: OwnerId, state: &mut ClusterState) -> Vec<ScheduledEvent> {
    let rs_id = match find_rs(state, owner_id) {
        Some(id) => id,
        None => return Vec::new(),
    };

    let rs = state.replica_sets.get(rs_id).unwrap().clone();
    let actual = state.count_owned_pods(owner_id);
    let desired = rs.desired_replicas;

    let follow_ups = Vec::new();

    if actual < desired {
        // Scale up
        let to_create = desired - actual;
        for _ in 0..to_create {
            let pod = Pod {
                requests: rs.pod_template.requests,
                limits: rs.pod_template.limits,
                phase: PodPhase::Pending,
                node: None,
                scheduling_constraints: rs.pod_template.scheduling_constraints.clone(),
                deletion_cost: None,
                owner: owner_id,
                qos_class: QoSClass::Burstable,
                priority: rs.pod_template.priority,
                labels: rs.pod_template.labels.clone(),
            };
            state.submit_pod(pod);
        }
    } else if actual > desired {
        // Scale down — select pods to delete
        let to_delete = (actual - desired) as usize;
        let mut running = state.running_pods_for_owner(owner_id);

        // Apply deletion-cost strategy before sorting
        if rs.deletion_cost_strategy != DeletionCostStrategy::None {
            // Costs are set by DeletionCostController for all strategies;
            // for PreferEmptyingNodes we also do an inline update for backward compat.
            if rs.deletion_cost_strategy == DeletionCostStrategy::PreferEmptyingNodes {
                update_deletion_costs(state, &running);
            }
        }

        // Sort: (a) deletion_cost ASC, (b) fewer co-located replicas, (c) newer first (higher index)
        running.sort_by(|a, b| {
            let pa = state.pods.get(*a).unwrap();
            let pb = state.pods.get(*b).unwrap();

            let cost_a = pa.deletion_cost.unwrap_or(0);
            let cost_b = pb.deletion_cost.unwrap_or(0);
            cost_a
                .cmp(&cost_b)
                .then_with(|| {
                    let coloc_a = colocated_count(state, pa, owner_id);
                    let coloc_b = colocated_count(state, pb, owner_id);
                    coloc_a.cmp(&coloc_b)
                })
                .then_with(|| b.index.cmp(&a.index)) // newer (higher index) first
        });

        for &pod_id in running.iter().take(to_delete) {
            state.remove_pod(pod_id);
        }
    }

    // Update deletion costs on remaining pods if strategy is active
    if rs.deletion_cost_strategy == DeletionCostStrategy::PreferEmptyingNodes {
        let running = state.running_pods_for_owner(owner_id);
        update_deletion_costs(state, &running);
    }

    follow_ups
}

/// Count how many sibling replicas (same owner) are co-located on the same node.
fn colocated_count(state: &ClusterState, pod: &Pod, owner: OwnerId) -> u32 {
    let node_id = match pod.node {
        Some(n) => n,
        None => return 0,
    };
    let node = match state.nodes.get(node_id) {
        Some(n) => n,
        None => return 0,
    };
    node.pods
        .iter()
        .filter(|&&pid| {
            state
                .pods
                .get(pid)
                .map_or(false, |p| p.owner == owner && p.phase == PodPhase::Running)
        })
        .count() as u32
}

/// Set deletion-cost = -(pods_remaining_on_node) for PreferEmptyingNodes strategy.
fn update_deletion_costs(state: &mut ClusterState, pod_ids: &[kubesim_core::PodId]) {
    // First pass: compute costs
    let costs: Vec<_> = pod_ids
        .iter()
        .map(|&pid| {
            let owner = state.pods.get(pid).unwrap().owner;
            let count = state
                .pods
                .get(pid)
                .and_then(|p| p.node)
                .and_then(|nid| state.nodes.get(nid))
                .map(|node| {
                    node.pods
                        .iter()
                        .filter(|&&other| {
                            state.pods.get(other).map_or(false, |p| {
                                p.owner == owner && p.phase == PodPhase::Running
                            })
                        })
                        .count() as i32
                })
                .unwrap_or(0);
            (pid, -count)
        })
        .collect();

    // Second pass: apply
    for (pid, cost) in costs {
        if let Some(pod) = state.pods.get_mut(pid) {
            pod.deletion_cost = Some(cost);
        }
    }
}
