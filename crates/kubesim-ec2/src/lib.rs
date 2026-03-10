//! KubeSim EC2 — EC2 instance type catalog with pricing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Spot price distribution parameters (normal approximation).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpotPriceDistribution {
    pub mean: f64,
    pub stddev: f64,
}

/// A single EC2 instance type entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceType {
    pub instance_type: String,
    pub vcpu: u32,
    pub memory_gib: u32,
    pub gpu_count: u32,
    pub gpu_type: Option<String>,
    pub network_bandwidth_gbps: f64,
    pub on_demand_price_per_hour: f64,
    #[serde(flatten)]
    pub spot: SpotPriceDistribution,
}

/// Flat JSON representation used for deserialization.
#[derive(Deserialize)]
struct RawEntry {
    instance_type: String,
    vcpu: u32,
    memory_gib: u32,
    gpu_count: u32,
    gpu_type: Option<String>,
    network_bandwidth_gbps: f64,
    on_demand_price_per_hour: f64,
    spot_mean: f64,
    spot_stddev: f64,
}

impl From<RawEntry> for InstanceType {
    fn from(r: RawEntry) -> Self {
        Self {
            instance_type: r.instance_type,
            vcpu: r.vcpu,
            memory_gib: r.memory_gib,
            gpu_count: r.gpu_count,
            gpu_type: r.gpu_type,
            network_bandwidth_gbps: r.network_bandwidth_gbps,
            on_demand_price_per_hour: r.on_demand_price_per_hour,
            spot: SpotPriceDistribution {
                mean: r.spot_mean,
                stddev: r.spot_stddev,
            },
        }
    }
}

/// Resource requirements filter for catalog queries.
pub struct ResourceFilter {
    pub min_vcpu: Option<u32>,
    pub min_memory_gib: Option<u32>,
    pub min_gpu: Option<u32>,
    pub gpu_type: Option<String>,
    pub max_on_demand_price: Option<f64>,
}

impl Default for ResourceFilter {
    fn default() -> Self {
        Self {
            min_vcpu: None,
            min_memory_gib: None,
            min_gpu: None,
            gpu_type: None,
            max_on_demand_price: None,
        }
    }
}

static EMBEDDED_CATALOG: &str = include_str!("catalog.json");

/// EC2 instance type catalog.
pub struct Catalog {
    types: Vec<InstanceType>,
    index: HashMap<String, usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("failed to parse catalog: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("failed to read catalog file: {0}")]
    Io(#[from] std::io::Error),
}

impl Catalog {
    /// Load from the embedded catalog.
    pub fn embedded() -> Result<Self, CatalogError> {
        Self::from_json(EMBEDDED_CATALOG)
    }

    /// Load from an external JSON file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, CatalogError> {
        let data = std::fs::read_to_string(path)?;
        Self::from_json(&data)
    }

    fn from_json(json: &str) -> Result<Self, CatalogError> {
        let raw: Vec<RawEntry> = serde_json::from_str(json)?;
        let types: Vec<InstanceType> = raw.into_iter().map(Into::into).collect();
        let index = types
            .iter()
            .enumerate()
            .map(|(i, t)| (t.instance_type.clone(), i))
            .collect();
        Ok(Self { types, index })
    }

    /// Look up an instance type by name (e.g. "m5.xlarge").
    pub fn get(&self, name: &str) -> Option<&InstanceType> {
        self.index.get(name).map(|&i| &self.types[i])
    }

    /// Return all instance types matching the given resource filter.
    pub fn filter(&self, f: &ResourceFilter) -> Vec<&InstanceType> {
        self.types
            .iter()
            .filter(|t| {
                f.min_vcpu.map_or(true, |v| t.vcpu >= v)
                    && f.min_memory_gib.map_or(true, |v| t.memory_gib >= v)
                    && f.min_gpu.map_or(true, |v| t.gpu_count >= v)
                    && f.gpu_type
                        .as_ref()
                        .map_or(true, |g| t.gpu_type.as_deref() == Some(g))
                    && f.max_on_demand_price
                        .map_or(true, |v| t.on_demand_price_per_hour <= v)
            })
            .collect()
    }

    /// Return all instance types in the catalog.
    pub fn all(&self) -> &[InstanceType] {
        &self.types
    }

    /// Number of instance types in the catalog.
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Whether the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}
