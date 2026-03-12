//! KubeSim EC2 — instance type catalog with pricing (EC2 and KWOK providers).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Catalog provider selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CatalogProvider {
    #[default]
    Ec2,
    Kwok,
}

/// Spot price distribution parameters (normal approximation).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SpotPriceDistribution {
    pub mean: f64,
    pub stddev: f64,
}

/// A single instance type entry.
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

/// Flat JSON representation used for EC2 deserialization.
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

// ── KWOK JSON format ────────────────────────────────────────────

#[derive(Deserialize)]
struct KwokEntry {
    name: String,
    offerings: Vec<KwokOffering>,
    resources: KwokResources,
}

#[derive(Deserialize)]
struct KwokOffering {
    #[serde(rename = "Price")]
    price: f64,
    #[serde(rename = "Requirements")]
    requirements: Vec<KwokRequirement>,
}

#[derive(Deserialize)]
struct KwokRequirement {
    key: String,
    values: Vec<String>,
}

#[derive(Deserialize)]
struct KwokResources {
    cpu: String,
    memory: String,
    #[serde(default, rename = "ephemeral-storage")]
    _ephemeral_storage: Option<String>,
    #[serde(default)]
    _pods: Option<String>,
}

fn parse_kwok_cpu(s: &str) -> u32 {
    if let Some(v) = s.strip_suffix('m') {
        v.parse::<u32>().unwrap_or(0) / 1000
    } else {
        s.parse::<u32>().unwrap_or(0)
    }
}

fn parse_kwok_memory_gib(s: &str) -> u32 {
    if let Some(v) = s.strip_suffix("Ti") {
        v.parse::<f64>().unwrap_or(0.0) as u32 * 1024
    } else if let Some(v) = s.strip_suffix("Gi") {
        v.parse::<f64>().unwrap_or(0.0) as u32
    } else if let Some(v) = s.strip_suffix("Mi") {
        (v.parse::<f64>().unwrap_or(0.0) / 1024.0) as u32
    } else {
        0
    }
}

impl From<KwokEntry> for InstanceType {
    fn from(k: KwokEntry) -> Self {
        let vcpu = parse_kwok_cpu(&k.resources.cpu);
        let memory_gib = parse_kwok_memory_gib(&k.resources.memory);

        // Extract on-demand and spot prices from offerings
        let mut on_demand_prices = Vec::new();
        let mut spot_prices = Vec::new();
        for o in &k.offerings {
            let cap_type = o.requirements.iter()
                .find(|r| r.key == "karpenter.sh/capacity-type")
                .and_then(|r| r.values.first())
                .map(|s| s.as_str());
            match cap_type {
                Some("on-demand") => on_demand_prices.push(o.price),
                Some("spot") => spot_prices.push(o.price),
                _ => {}
            }
        }

        let on_demand = on_demand_prices.first().copied().unwrap_or(0.0);
        let spot_mean = if spot_prices.is_empty() {
            on_demand * 0.7
        } else {
            spot_prices.iter().sum::<f64>() / spot_prices.len() as f64
        };

        Self {
            instance_type: k.name,
            vcpu,
            memory_gib,
            gpu_count: 0,
            gpu_type: None,
            network_bandwidth_gbps: 10.0,
            on_demand_price_per_hour: on_demand,
            spot: SpotPriceDistribution {
                mean: spot_mean,
                stddev: spot_mean * 0.1,
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

static EMBEDDED_EC2_CATALOG: &str = include_str!("catalog.json");
static EMBEDDED_KWOK_CATALOG: &str = include_str!("kwok_instance_types.json");

/// Instance type catalog.
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
    /// Load from the embedded EC2 catalog (default).
    pub fn embedded() -> Result<Self, CatalogError> {
        Self::from_ec2_json(EMBEDDED_EC2_CATALOG)
    }

    /// Load from the embedded KWOK catalog.
    pub fn kwok() -> Result<Self, CatalogError> {
        Self::from_kwok_json(EMBEDDED_KWOK_CATALOG)
    }

    /// Load the catalog for the given provider.
    pub fn for_provider(provider: CatalogProvider) -> Result<Self, CatalogError> {
        match provider {
            CatalogProvider::Ec2 => Self::embedded(),
            CatalogProvider::Kwok => Self::kwok(),
        }
    }

    /// Load from an external JSON file.
    pub fn from_file(path: &std::path::Path) -> Result<Self, CatalogError> {
        let data = std::fs::read_to_string(path)?;
        Self::from_ec2_json(&data)
    }

    fn from_ec2_json(json: &str) -> Result<Self, CatalogError> {
        let raw: Vec<RawEntry> = serde_json::from_str(json)?;
        let types: Vec<InstanceType> = raw.into_iter().map(Into::into).collect();
        Self::from_types(types)
    }

    fn from_kwok_json(json: &str) -> Result<Self, CatalogError> {
        let raw: Vec<KwokEntry> = serde_json::from_str(json)?;
        let types: Vec<InstanceType> = raw.into_iter().map(Into::into).collect();
        Self::from_types(types)
    }

    fn from_types(types: Vec<InstanceType>) -> Result<Self, CatalogError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kwok_catalog_loads_and_parses() {
        let catalog = Catalog::kwok().expect("KWOK catalog should load");
        assert!(catalog.len() > 0, "KWOK catalog should not be empty");

        // Verify a known instance type
        let it = catalog.get("c-1x-amd64-linux").expect("c-1x-amd64-linux should exist");
        assert_eq!(it.vcpu, 1);
        assert_eq!(it.memory_gib, 2);
        assert!(it.on_demand_price_per_hour > 0.0);
        assert!(it.spot.mean > 0.0);
    }

    #[test]
    fn ec2_catalog_loads() {
        let catalog = Catalog::embedded().expect("EC2 catalog should load");
        assert!(catalog.len() > 0);
        assert!(catalog.get("m5.xlarge").is_some());
    }

    #[test]
    fn for_provider_selects_correct_catalog() {
        let ec2 = Catalog::for_provider(CatalogProvider::Ec2).unwrap();
        let kwok = Catalog::for_provider(CatalogProvider::Kwok).unwrap();
        assert!(ec2.get("m5.xlarge").is_some());
        assert!(ec2.get("c-1x-amd64-linux").is_none());
        assert!(kwok.get("c-1x-amd64-linux").is_some());
        assert!(kwok.get("m5.xlarge").is_none());
    }
}
