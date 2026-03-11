//! Karpenter Provisioner — batches pending pods and selects cheapest instance types.

use kubesim_core::*;
use kubesim_ec2::Catalog;

use crate::nodepool::{NodePool, NodePoolUsage};

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
pub fn batch_pending_pods(state: &ClusterState) -> Vec<PodBatch> {
    use std::collections::HashMap;

    // Key: (sorted required labels, sorted toleration keys)
    let mut groups: HashMap<(Vec<(String, String)>, Vec<String>, u32), Vec<PodId>> = HashMap::new();

    for &pod_id in &state.pending_queue {
        let pod = match state.pods.get(pod_id) {
            Some(p) if p.phase == PodPhase::Pending => p,
            _ => continue,
        };

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
    let batches = batch_pending_pods(state);
    let mut decisions = Vec::new();
    let mut running_usage = usage.clone();

    for batch in &batches {
        if let Some(decision) = select_instance(batch, catalog, pool, &running_usage) {
            // Update running usage for subsequent batches
            if let Some(it) = catalog.get(&decision.instance_type) {
                running_usage.node_count += 1;
                running_usage.cpu_millis += (it.vcpu as u64) * 1000;
                running_usage.memory_bytes += (it.memory_gib as u64) * 1024 * 1024 * 1024;
            }
            decisions.push(decision);
        }
    }
    decisions
}
