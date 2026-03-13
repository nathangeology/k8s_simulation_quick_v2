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
    select_instance_with_overhead(batch, catalog, pool, usage, &Resources::default(), 0)
}

/// Select the cheapest instance type, accounting for system overhead.
pub fn select_instance_with_overhead(
    batch: &PodBatch,
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
    overhead: &Resources,
    daemonset_pct: u32,
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
        let raw_cpu = (it.vcpu as u64) * 1000;
        let raw_mem = (it.memory_gib as u64) * 1024 * 1024 * 1024;
        let (oh_cpu, oh_mem) = if overhead.cpu_millis == 0 && overhead.memory_bytes == 0 {
            kubesim_ec2::eks_overhead(it.vcpu)
        } else {
            (overhead.cpu_millis, overhead.memory_bytes)
        };
        let mut it_cpu = raw_cpu.saturating_sub(oh_cpu);
        let mut it_mem = raw_mem.saturating_sub(oh_mem);
        if daemonset_pct > 0 {
            it_cpu = it_cpu.saturating_sub(raw_cpu * daemonset_pct as u64 / 100);
            it_mem = it_mem.saturating_sub(raw_mem * daemonset_pct as u64 / 100);
        }

        if it_cpu < cpu_needed || it_mem < mem_needed || it.gpu_count < gpu_needed {
            continue;
        }
        if !pool.can_launch(usage, raw_cpu, raw_mem) {
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
    provision_versioned(state, catalog, pool, usage, None, &Resources::default(), 0)
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
/// v1.x: cost-optimizing bin-packing like real Karpenter — evaluates all
///        candidate instance types by cost-per-pod ratio (price / pods_that_fit),
///        picks the best-scoring type, assigns pods, and repeats. Ties broken
///        by preferring the instance that fits more pods (fewer, larger nodes).
pub fn provision_versioned(
    state: &ClusterState,
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
    profile: Option<&VersionProfile>,
    overhead: &Resources,
    daemonset_pct: u32,
) -> Vec<ProvisionDecision> {
    let batches = batch_pending_pods(state, Some(pool));
    // Use cost-optimizing provisioner by default (matches Karpenter v1.x).
    // Only fall back to legacy FFD for explicit v0.35 profiles.
    let use_cost_opt = profile.map_or(true, |p| p.version != KarpenterVersion::V0_35);

    let mut decisions = Vec::new();
    let mut running_usage = usage.clone();

    for batch in &batches {
        if use_cost_opt {
            // Karpenter-style cost-optimizing provisioner: for each candidate
            // instance type, compute how many pods fit and score by cost-per-pod.
            // Greedily pick the best-scoring type, assign pods, repeat.
            let mut pods: Vec<(PodId, Resources)> = batch.pod_ids.iter().filter_map(|&pid| {
                state.pods.get(pid).map(|p| (pid, p.requests))
            }).collect();
            // Sort largest-first so greedy packing fills big pods first
            pods.sort_by(|a, b| {
                let sa = a.1.cpu_millis + a.1.memory_bytes / 1_000_000;
                let sb = b.1.cpu_millis + b.1.memory_bytes / 1_000_000;
                sb.cmp(&sa)
            });

            let mut remaining = pods;
            while !remaining.is_empty() {
                if !pool.can_launch(&running_usage, 0, 0) { break; }

                // Score each allowed instance type by cost-per-pod
                let best = score_best_instance(
                    catalog, pool, &running_usage, &remaining,
                    batch.gpu_required, overhead, daemonset_pct,
                );
                let Some((it_name, price, fit_count)) = best else { break };

                let assigned: Vec<PodId> = remaining.iter().take(fit_count).map(|&(pid, _)| pid).collect();
                remaining = remaining.split_off(fit_count);

                if let Some(spec) = catalog.get(&it_name) {
                    running_usage.node_count += 1;
                    running_usage.cpu_millis += (spec.vcpu as u64) * 1000;
                    running_usage.memory_bytes += (spec.memory_gib as u64) * 1024 * 1024 * 1024;
                }
                decisions.push(ProvisionDecision {
                    instance_type: it_name,
                    cost_per_hour: price,
                    pod_ids: assigned,
                });
            }
        } else {
            // Legacy v0.35: try to fit entire batch on one cheapest instance
            if let Some(decision) = select_instance_with_overhead(batch, catalog, pool, &running_usage, overhead, daemonset_pct) {
                if let Some(it) = catalog.get(&decision.instance_type) {
                    running_usage.node_count += 1;
                    running_usage.cpu_millis += (it.vcpu as u64) * 1000;
                    running_usage.memory_bytes += (it.memory_gib as u64) * 1024 * 1024 * 1024;
                }
                decisions.push(decision);
            } else if batch.pod_ids.len() > 1 {
                // Batch too large for any single instance — greedy fill with largest.
                let largest = find_largest_instance(catalog, pool, &running_usage, overhead, daemonset_pct);
                let Some(largest_it) = largest else { continue };
                let mut remaining_cpu = effective_cpu(largest_it, overhead, daemonset_pct);
                let mut remaining_mem = effective_mem(largest_it, overhead, daemonset_pct);
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
                        let Some(next_it) = find_largest_instance(catalog, pool, &running_usage, overhead, daemonset_pct) else { break };
                        remaining_cpu = effective_cpu(next_it, overhead, daemonset_pct);
                        remaining_mem = effective_mem(next_it, overhead, daemonset_pct);
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
    overhead: &Resources,
    daemonset_pct: u32,
) -> Option<(String, f64)> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter().filter_map(|n| catalog.get(n)).collect()
    };
    let mut best: Option<(&kubesim_ec2::InstanceType, f64)> = None;
    for it in &allowed {
        let it_cpu = effective_cpu(it, overhead, daemonset_pct);
        let it_mem = effective_mem(it, overhead, daemonset_pct);
        if it_cpu < needed.cpu_millis || it_mem < needed.memory_bytes || it.gpu_count < gpu_needed {
            continue;
        }
        if !pool.can_launch(usage, (it.vcpu as u64) * 1000, (it.memory_gib as u64) * 1024 * 1024 * 1024) {
            continue;
        }
        let price = it.on_demand_price_per_hour;
        if best.as_ref().map_or(true, |(_, bp)| price < *bp) {
            best = Some((it, price));
        }
    }
    best.map(|(it, price)| (it.instance_type.clone(), price))
}

/// Score all candidate instance types by cost-per-pod and return the best one.
/// For each instance type, greedily counts how many pods (sorted largest-first)
/// fit, then scores as price / pods_that_fit. Returns (instance_type, price, fit_count).
fn score_best_instance(
    catalog: &Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
    pods: &[(PodId, Resources)],
    gpu_needed: u32,
    overhead: &Resources,
    daemonset_pct: u32,
) -> Option<(String, f64, usize)> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter().filter_map(|n| catalog.get(n)).collect()
    };

    let mut best: Option<(f64, &kubesim_ec2::InstanceType, usize)> = None; // (score, it, count)

    for it in &allowed {
        if it.gpu_count < gpu_needed { continue; }
        let raw_cpu = (it.vcpu as u64) * 1000;
        let raw_mem = (it.memory_gib as u64) * 1024 * 1024 * 1024;
        if !pool.can_launch(usage, raw_cpu, raw_mem) { continue; }

        let avail_cpu = effective_cpu(it, overhead, daemonset_pct);
        let avail_mem = effective_mem(it, overhead, daemonset_pct);

        // Greedily pack pods (already sorted largest-first)
        let mut used_cpu: u64 = 0;
        let mut used_mem: u64 = 0;
        let mut count = 0usize;
        for &(_, ref req) in pods {
            if used_cpu + req.cpu_millis <= avail_cpu && used_mem + req.memory_bytes <= avail_mem {
                used_cpu += req.cpu_millis;
                used_mem += req.memory_bytes;
                count += 1;
            }
        }
        if count == 0 { continue; }

        let score = it.on_demand_price_per_hour / count as f64;
        let better = best.as_ref().map_or(true, |(bs, _, bc)| {
            score < *bs || (score == *bs && count > *bc)
        });
        if better {
            best = Some((score, it, count));
        }
    }

    best.map(|(_, it, count)| (it.instance_type.clone(), it.on_demand_price_per_hour, count))
}

/// Find the largest allowed instance type that can still be launched.
fn find_largest_instance<'a>(
    catalog: &'a Catalog,
    pool: &NodePool,
    usage: &NodePoolUsage,
    overhead: &Resources,
    daemonset_pct: u32,
) -> Option<&'a kubesim_ec2::InstanceType> {
    let allowed: Vec<&kubesim_ec2::InstanceType> = if pool.instance_types.is_empty() {
        catalog.all().iter().collect()
    } else {
        pool.instance_types.iter().filter_map(|n| catalog.get(n)).collect()
    };
    allowed.into_iter()
        .filter(|it| pool.can_launch(usage, (it.vcpu as u64) * 1000, (it.memory_gib as u64) * 1024 * 1024 * 1024))
        .max_by_key(|it| {
            let eff_cpu = effective_cpu(it, overhead, daemonset_pct);
            let eff_mem = effective_mem(it, overhead, daemonset_pct);
            eff_cpu + eff_mem / 1_000_000
        })
}

/// Effective allocatable CPU for an instance type after overhead.
fn effective_cpu(it: &kubesim_ec2::InstanceType, overhead: &Resources, daemonset_pct: u32) -> u64 {
    let raw = (it.vcpu as u64) * 1000;
    let oh_cpu = if overhead.cpu_millis == 0 && overhead.memory_bytes == 0 {
        kubesim_ec2::eks_overhead(it.vcpu).0
    } else {
        overhead.cpu_millis
    };
    let mut eff = raw.saturating_sub(oh_cpu);
    if daemonset_pct > 0 { eff = eff.saturating_sub(raw * daemonset_pct as u64 / 100); }
    eff
}

/// Effective allocatable memory for an instance type after overhead.
fn effective_mem(it: &kubesim_ec2::InstanceType, overhead: &Resources, daemonset_pct: u32) -> u64 {
    let raw = (it.memory_gib as u64) * 1024 * 1024 * 1024;
    let oh_mem = if overhead.cpu_millis == 0 && overhead.memory_bytes == 0 {
        kubesim_ec2::eks_overhead(it.vcpu).1
    } else {
        overhead.memory_bytes
    };
    let mut eff = raw.saturating_sub(oh_mem);
    if daemonset_pct > 0 { eff = eff.saturating_sub(raw * daemonset_pct as u64 / 100); }
    eff
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
            duration_ns: None, is_daemonset: false,
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
