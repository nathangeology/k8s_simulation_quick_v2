//! Karpenter Provisioner — batches pending pods and selects cheapest instance types.

use kubesim_core::*;
use kubesim_ec2::Catalog;

use crate::nodepool::{NodePool, NodePoolUsage};
use crate::version::{KarpenterVersion, VersionProfile};

/// A batch of compatible pending pods that can share a single node.
#[derive(Debug)]
pub struct PodBatch {
    pub pod_ids: Vec<PodId>,
    /// Aggregate resource requirements for the batch.
    pub total_requests: Resources,
    /// Required node labels (intersection of nodeSelector / required node affinity).
    pub required_labels: Vec<(String, String)>,
    /// Tolerations that ALL pods in the batch share.
    pub common_tolerations: Vec<Toleration>,
    /// Minimum GPU requirement across the batch.
    pub gpu_required: u32,
}

/// Result of a provisioning decision for one batch.
#[derive(Debug)]
pub struct ProvisionDecision {
    pub instance_type: String,
    pub cost_per_hour: f64,
    pub pod_ids: Vec<PodId>,
}

/// Batch pending pods by compatible scheduling constraints.
///
/// Two pods are compatible if they share the same required node labels and
/// the same set of tolerations. This is a simplified model — real Karpenter
/// uses a more nuanced grouping.
///
/// When `pool` is provided, only pods matching the pool's labels/taints are included.
pub fn batch_pending_pods(state: &ClusterState, pool: Option<&NodePool>) -> Vec<PodBatch> {
    use std::collections::HashMap;

    // Key: (sorted required labels, sorted toleration keys)
    let mut groups: HashMap<(Vec<(String, String)>, Vec<String>, u32), Vec<PodId>> = HashMap::new();

    for &pod_id in &state.pending_queue {
        let pod = match state.pods.get(pod_id) {
            Some(p) if p.phase == PodPhase::Pending => p,
            _ => continue,
        };

        // Filter by pool compatibility if a pool is provided
        if let Some(pool) = pool {
            if !pod_matches_pool(pod, pool) {
                continue;
            }
        }

        let mut req_labels: Vec<(String, String)> = Vec::new();
        for term in &pod.scheduling_constraints.node_affinity {
            if matches!(term.affinity_type, AffinityType::Required) {
                req_labels.extend(term.match_labels.0.iter().cloned());
            }
        }
        req_labels.sort();
        req_labels.dedup();

        let mut tol_keys: Vec<String> = pod.scheduling_constraints.tolerations
            .iter()
            .map(|t| t.key.clone())
            .collect();
        tol_keys.sort();

        let gpu = pod.requests.gpu;
        let key = (req_labels, tol_keys, gpu);
        groups.entry(key).or_default().push(pod_id);
    }

    groups.into_iter().map(|((req_labels, _, gpu), pod_ids)| {
        let mut total = Resources::default();
        let mut common_tolerations = Vec::new();
        let mut first = true;

        for &pid in &pod_ids {
            if let Some(p) = state.pods.get(pid) {
                total = total.saturating_add(&p.requests);
                if first {
                    common_tolerations = p.scheduling_constraints.tolerations.clone();
                    first = false;
                }
            }
        }

        PodBatch {
            pod_ids,
            total_requests: total,
            required_labels: req_labels,
            common_tolerations,
            gpu_required: gpu,
        }
    }).collect()
}

/// Select the cheapest instance type from the catalog that fits a batch,
/// respecting NodePool constraints.
pub fn select_instance(
    batch: &PodBatch,
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
) -> Option<ProvisionDecision> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter()
            .filter_map(|name| catalog.get(name))
            .collect()
    };

    let cpu_needed = batch.total_requests.cpu_millis;
    let mem_needed = batch.total_requests.memory_bytes;
    let gpu_needed = batch.gpu_required;

    let mut best: Option<(&kubesim_ec2::InstanceType, f64)> = None;

    for it in &allowed {
        let it_cpu = (it.vcpu as u64) * 1000;
        let it_mem = (it.memory_gib as u64) * 1024 * 1024 * 1024;

        if it_cpu < cpu_needed || it_mem < mem_needed || it.gpu_count < gpu_needed {
            continue;
        }
        if !pool.can_launch(usage, it_cpu, it_mem) {
            continue;
        }

        let price = it.on_demand_price_per_hour;
        if best.as_ref().map_or(true, |(_, bp)| price < *bp) {
            best = Some((it, price));
        }
    }

    best.map(|(it, price)| ProvisionDecision {
        instance_type: it.instance_type.clone(),
        cost_per_hour: price,
        pod_ids: batch.pod_ids.clone(),
    })
}

/// Run one provisioning loop: batch pending pods, select instances, return decisions.
pub fn provision(
    state: &ClusterState,
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
) -> Vec<ProvisionDecision> {
    provision_versioned(state, catalog, pool, usage, None)
}

/// Check whether a pending pod is compatible with a pool's labels and taints.
/// A pod matches a pool if:
/// - The pod's required nodeAffinity labels are a subset of the pool's labels
///   (or the pod has no required labels)
/// - The pod tolerates all of the pool's taints
fn pod_matches_pool(pod: &Pod, pool: &NodePool) -> bool {
    // Check taints: pod must tolerate every pool taint
    for taint in &pool.taints {
        if !pod.scheduling_constraints.tolerations.iter().any(|t| t.tolerates(taint)) {
            return false;
        }
    }
    // Check labels: if pool has labels, pod's required nodeAffinity labels must match
    if !pool.labels.is_empty() {
        let pool_labels = LabelSet(pool.labels.clone());
        for term in &pod.scheduling_constraints.node_affinity {
            if matches!(term.affinity_type, AffinityType::Required) {
                if !term.match_labels.0.iter().all(|(k, v)| pool_labels.get(k) == Some(v.as_str())) {
                    return false;
                }
            }
        }
    }
    true
}

/// Version-aware provisioning.
///
/// v0.35: cheapest-fit per batch (original behavior).
/// v1.x: first-fit-decreasing bin-packing like real Karpenter — sorts pods
///        largest-first, packs onto virtual nodes, then right-sizes each node
///        to the cheapest instance type that fits, producing a heterogeneous fleet.
pub fn provision_versioned(
    state: &ClusterState,
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
    profile: Option<&VersionProfile>,
) -> Vec<ProvisionDecision> {
    let batches = batch_pending_pods(state, Some(pool));
    let use_ffd = profile.map_or(false, |p| p.version == KarpenterVersion::V1);

    let mut decisions = Vec::new();
    let mut running_usage = usage.clone();

    for batch in &batches {
        if use_ffd {
            // FFD bin-packing: sort pods largest-first, pack onto virtual nodes,
            // right-size each node to cheapest fitting instance.
            let mut pods: Vec<(PodId, Resources)> = batch.pod_ids.iter().filter_map(|&pid| {
                state.pods.get(pid).map(|p| (pid, p.requests))
            }).collect();
            pods.sort_by(|a, b| {
                let sa = a.1.cpu_millis + a.1.memory_bytes / 1_000_000;
                let sb = b.1.cpu_millis + b.1.memory_bytes / 1_000_000;
                sb.cmp(&sa)
            });

            let mut node_pods: Vec<PodId> = Vec::new();
            let mut node_total = Resources::default();

            for (pid, req) in pods {
                let combined = node_total.saturating_add(&req);
                // Check if any instance can fit the combined load
                if !node_pods.is_empty() && cheapest_fit(catalog, pool, &running_usage, &combined, batch.gpu_required).is_none() {
                    // Right-size and flush current node
                    if let Some((it, price)) = cheapest_fit(catalog, pool, &running_usage, &node_total, batch.gpu_required) {
                        if let Some(spec) = catalog.get(&it) {
                            running_usage.node_count += 1;
                            running_usage.cpu_millis += (spec.vcpu as u64) * 1000;
                            running_usage.memory_bytes += (spec.memory_gib as u64) * 1024 * 1024 * 1024;
                        }
                        decisions.push(ProvisionDecision {
                            instance_type: it,
                            cost_per_hour: price,
                            pod_ids: std::mem::take(&mut node_pods),
                        });
                    }
                    node_total = Resources::default();
                    if !pool.can_launch(&running_usage, 0, 0) { break; }
                }
                node_total = node_total.saturating_add(&req);
                node_pods.push(pid);
            }
            // Flush last node
            if !node_pods.is_empty() {
                if let Some((it, price)) = cheapest_fit(catalog, pool, &running_usage, &node_total, batch.gpu_required) {
                    if let Some(spec) = catalog.get(&it) {
                        running_usage.node_count += 1;
                        running_usage.cpu_millis += (spec.vcpu as u64) * 1000;
                        running_usage.memory_bytes += (spec.memory_gib as u64) * 1024 * 1024 * 1024;
                    }
                    decisions.push(ProvisionDecision {
                        instance_type: it,
                        cost_per_hour: price,
                        pod_ids: node_pods,
                    });
                }
            }
        } else {
            // Legacy v0.35: try to fit entire batch on one cheapest instance
            if let Some(decision) = select_instance(batch, catalog, pool, &running_usage) {
                if let Some(it) = catalog.get(&decision.instance_type) {
                    running_usage.node_count += 1;
                    running_usage.cpu_millis += (it.vcpu as u64) * 1000;
                    running_usage.memory_bytes += (it.memory_gib as u64) * 1024 * 1024 * 1024;
                }
                decisions.push(decision);
            } else if batch.pod_ids.len() > 1 {
                // Batch too large for any single instance — greedy fill with largest.
                let largest = find_largest_instance(catalog, pool, &running_usage);
                let Some(largest_it) = largest else { continue };
                let mut remaining_cpu = (largest_it.vcpu as u64) * 1000;
                let mut remaining_mem = (largest_it.memory_gib as u64) * 1024 * 1024 * 1024;
                let mut current_pods: Vec<PodId> = Vec::new();

                for &pid in &batch.pod_ids {
                    let Some(pod) = state.pods.get(pid) else { continue };
                    let pcpu = pod.requests.cpu_millis;
                    let pmem = pod.requests.memory_bytes;

                    if pcpu > remaining_cpu || pmem > remaining_mem {
                        if !current_pods.is_empty() {
                            if let Some(it) = catalog.get(&largest_it.instance_type) {
                                running_usage.node_count += 1;
                                running_usage.cpu_millis += (it.vcpu as u64) * 1000;
                                running_usage.memory_bytes += (it.memory_gib as u64) * 1024 * 1024 * 1024;
                            }
                            decisions.push(ProvisionDecision {
                                instance_type: largest_it.instance_type.clone(),
                                cost_per_hour: largest_it.on_demand_price_per_hour,
                                pod_ids: std::mem::take(&mut current_pods),
                            });
                            if !pool.can_launch(&running_usage, 0, 0) { break; }
                        }
                        let Some(next_it) = find_largest_instance(catalog, pool, &running_usage) else { break };
                        remaining_cpu = (next_it.vcpu as u64) * 1000;
                        remaining_mem = (next_it.memory_gib as u64) * 1024 * 1024 * 1024;
                    }
                    remaining_cpu = remaining_cpu.saturating_sub(pcpu);
                    remaining_mem = remaining_mem.saturating_sub(pmem);
                    current_pods.push(pid);
                }
                if !current_pods.is_empty() {
                    if let Some(it) = catalog.get(&largest_it.instance_type) {
                        running_usage.node_count += 1;
                        running_usage.cpu_millis += (it.vcpu as u64) * 1000;
                        running_usage.memory_bytes += (it.memory_gib as u64) * 1024 * 1024 * 1024;
                    }
                    decisions.push(ProvisionDecision {
                        instance_type: largest_it.instance_type.clone(),
                        cost_per_hour: largest_it.on_demand_price_per_hour,
                        pod_ids: current_pods,
                    });
                }
            }
        }
    }
    decisions
}

/// Select the best pool from multiple pools using v1.x weight-based priority.
/// Higher weight = higher priority. Returns pools sorted by descending weight.
pub fn sort_pools_by_weight(pools: &mut [&NodePool]) {
    pools.sort_by(|a, b| b.weight.cmp(&a.weight));
}

/// Find the cheapest instance type that fits the given resource requirements.
/// Returns `(instance_type_name, price)` or `None`.
fn cheapest_fit(
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
    needed: &Resources,
    gpu_needed: u32,
) -> Option<(String, f64)> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter().filter_map(|n| catalog.get(n)).collect()
    };
    let mut best: Option<(&kubesim_ec2::InstanceType, f64)> = None;
    for it in &allowed {
        let it_cpu = (it.vcpu as u64) * 1000;
        let it_mem = (it.memory_gib as u64) * 1024 * 1024 * 1024;
        if it_cpu < needed.cpu_millis || it_mem < needed.memory_bytes || it.gpu_count < gpu_needed {
            continue;
        }
        if !pool.can_launch(usage, it_cpu, it_mem) {
            continue;
        }
        let price = it.on_demand_price_per_hour;
        if best.as_ref().map_or(true, |(_, bp)| price < *bp) {
            best = Some((it, price));
        }
    }
    best.map(|(it, price)| (it.instance_type.clone(), price))
}

/// Find the largest allowed instance type that can still be launched.
fn find_largest_instance<'a>(
    catalog: &'a Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
) -> Option<&'a kubesim_ec2::InstanceType> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter().filter_map(|n| catalog.get(n)).collect()
    };
    allowed.into_iter()
        .filter(|it| pool.can_launch(usage, (it.vcpu as u64) * 1000, (it.memory_gib as u64) * 1024 * 1024 * 1024))
        .max_by_key(|it| (it.vcpu as u64) * 1000 + it.memory_gib as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodepool::NodePoolLimits;

    fn test_pod(cpu: u64, mem: u64) -> Pod {
        Pod {
            requests: Resources { cpu_millis: cpu, memory_bytes: mem, gpu: 0, ephemeral_bytes: 0 },
            limits: Resources::default(),
            phase: PodPhase::Pending,
            node: None,
            scheduling_constraints: SchedulingConstraints::default(),
            deletion_cost: None,
            owner: OwnerId(0),
            qos_class: QoSClass::Burstable,
            priority: 0,
            labels: LabelSet::default(),
            do_not_disrupt: false,
            duration_ns: None,
        }
    }

    fn test_pool() -> NodePool {
        NodePool {
            name: "default".into(),
            instance_types: vec!["m5.xlarge".into(), "m5.2xlarge".into()],
            limits: NodePoolLimits::default(),
            labels: vec![],
            taints: vec![],
            max_disrupted_pct: 10,
            max_disrupted_count: None,
            weight: 0,
            do_not_disrupt: false,
        }
    }

    fn test_catalog() -> kubesim_ec2::Catalog {
        kubesim_ec2::Catalog::embedded().unwrap()
    }

    #[test]
    fn batch_pending_pods_groups_by_constraints() {
        let mut state = ClusterState::new();
        // Two pods with same constraints → one batch
        state.submit_pod(test_pod(500, 500_000_000));
        state.submit_pod(test_pod(500, 500_000_000));

        let batches = batch_pending_pods(&state, None);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].pod_ids.len(), 2);
        assert_eq!(batches[0].total_requests.cpu_millis, 1000);
    }

    #[test]
    fn batch_pending_pods_separates_different_affinities() {
        let mut state = ClusterState::new();
        state.submit_pod(test_pod(500, 500_000_000));

        let mut pod2 = test_pod(500, 500_000_000);
        pod2.scheduling_constraints.node_affinity.push(NodeAffinityTerm {
            affinity_type: AffinityType::Required,
            match_labels: LabelSet(vec![("zone".into(), "us-east-1a".into())]),
        });
        state.submit_pod(pod2);

        let batches = batch_pending_pods(&state, None);
        assert_eq!(batches.len(), 2);
    }

    #[test]
    fn select_instance_picks_cheapest_fit() {
        let catalog = test_catalog();
        let pool = test_pool();
        let usage = NodePoolUsage::default();
        let batch = PodBatch {
            pod_ids: vec![],
            total_requests: Resources { cpu_millis: 2000, memory_bytes: 4_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            required_labels: vec![],
            common_tolerations: vec![],
            gpu_required: 0,
        };

        let decision = select_instance(&batch, &catalog, &pool, &usage);
        assert!(decision.is_some());
        let d = decision.unwrap();
        // m5.xlarge (4 vcpu, 16 GiB) is cheaper than m5.2xlarge
        assert_eq!(d.instance_type, "m5.xlarge");
    }

    #[test]
    fn select_instance_respects_pool_limits() {
        let catalog = test_catalog();
        let pool = NodePool {
            limits: NodePoolLimits { max_nodes: Some(1), ..Default::default() },
            ..test_pool()
        };
        let usage = NodePoolUsage { node_count: 1, ..Default::default() };
        let batch = PodBatch {
            pod_ids: vec![],
            total_requests: Resources { cpu_millis: 1000, memory_bytes: 1_000_000_000, gpu: 0, ephemeral_bytes: 0 },
            required_labels: vec![],
            common_tolerations: vec![],
            gpu_required: 0,
        };

        let decision = select_instance(&batch, &catalog, &pool, &usage);
        assert!(decision.is_none());
    }

    #[test]
    fn provision_returns_decisions_for_pending_pods() {
        let mut state = ClusterState::new();
        state.submit_pod(test_pod(1000, 1_000_000_000));
        state.submit_pod(test_pod(1000, 1_000_000_000));

        let catalog = test_catalog();
        let pool = test_pool();
        let usage = NodePoolUsage::default();

        let decisions = provision(&state, &catalog, &pool, &usage);
        assert!(!decisions.is_empty());
    }

    #[test]
    fn provision_empty_queue_returns_empty() {
        let state = ClusterState::new();
        let catalog = test_catalog();
        let pool = test_pool();
        let usage = NodePoolUsage::default();

        let decisions = provision(&state, &catalog, &pool, &usage);
        assert!(decisions.is_empty());
    }
}
