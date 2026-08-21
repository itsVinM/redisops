use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::query::{Aggregation, QueryResult, TimeRange};
use super::retention::RetentionPolicy;
use crate::btree::storage::Storage;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: u64,
    pub value: f64,
    pub labels: HashMap<String, String>,
}

#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub points: Vec<DataPoint>,
    pub retention: Option<RetentionPolicy>,
}

pub struct TimeSeriesDB {
    storage: Arc<Storage>,
    series: Arc<RwLock<HashMap<String, Series>>>,
}

impl TimeSeriesDB {
    pub fn new(storage: Arc<Storage>) -> Self {
        TimeSeriesDB {
            storage,
            series: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn init(&self) -> Result<(), String> {
        // Load series metadata from storage
        let keys = self.storage.scan(b"ts:meta:", b"ts:meta:\xff").await?;
        let mut series = self.series.write().await;

        for (key, value) in keys {
            let name = String::from_utf8_lossy(&key[9..]).to_string();
            let points: Vec<DataPoint> = serde_json::from_slice(&value).unwrap_or_default();
            series.insert(
                name.clone(),
                Series {
                    name,
                    points,
                    retention: None,
                },
            );
        }

        Ok(())
    }

    pub async fn add(
        &self,
        name: &str,
        timestamp: u64,
        value: f64,
        labels: HashMap<String, String>,
    ) -> Result<(), String> {
        let point = DataPoint {
            timestamp,
            value,
            labels,
        };

        let mut series = self.series.write().await;
        let s = series.entry(name.to_string()).or_insert_with(|| Series {
            name: name.to_string(),
            points: Vec::new(),
            retention: None,
        });

        s.points.push(point);
        s.points.sort_by_key(|p| p.timestamp);

        // Apply retention policy
        if let Some(ref policy) = s.retention {
            let cutoff = timestamp.saturating_sub(policy.max_age_secs * 1000);
            s.points.retain(|p| p.timestamp >= cutoff);
        }

        // Persist to storage
        let data = serde_json::to_vec(&s.points).unwrap_or_default();
        self.storage
            .put(format!("ts:meta:{}", name).as_bytes(), &data)
            .await?;

        Ok(())
    }

    pub async fn query(
        &self,
        name: &str,
        range: TimeRange,
        aggregation: Aggregation,
    ) -> Result<QueryResult, String> {
        let series = self.series.read().await;
        let s = series
            .get(name)
            .ok_or_else(|| format!("series '{}' not found", name))?;

        let points: Vec<&DataPoint> = s
            .points
            .iter()
            .filter(|p| p.timestamp >= range.start && p.timestamp <= range.end)
            .collect();

        let values: Vec<f64> = points.iter().map(|p| p.value).collect();

        let result_value = match aggregation {
            Aggregation::Sum => values.iter().sum(),
            Aggregation::Avg => {
                if values.is_empty() {
                    0.0
                } else {
                    values.iter().sum::<f64>() / values.len() as f64
                }
            }
            Aggregation::Min => values.iter().cloned().fold(f64::INFINITY, f64::min),
            Aggregation::Max => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            Aggregation::Count => values.len() as f64,
            Aggregation::Last => values.last().copied().unwrap_or(0.0),
        };

        Ok(QueryResult {
            name: name.to_string(),
            range,
            aggregation,
            value: result_value,
            points: points.into_iter().cloned().collect(),
        })
    }

    pub async fn set_retention(&self, name: &str, policy: RetentionPolicy) -> Result<(), String> {
        let mut series = self.series.write().await;
        let s = series
            .get_mut(name)
            .ok_or_else(|| format!("series '{}' not found", name))?;
        s.retention = Some(policy);
        Ok(())
    }

    pub async fn list_series(&self) -> Vec<String> {
        let series = self.series.read().await;
        series.keys().cloned().collect()
    }

    pub async fn point_count(&self, name: &str) -> usize {
        let series = self.series.read().await;
        series.get(name).map(|s| s.points.len()).unwrap_or(0)
    }

    pub async fn total_points(&self) -> usize {
        let series = self.series.read().await;
        series.values().map(|s| s.points.len()).sum()
    }
}
