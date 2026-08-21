use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetentionPolicy {
    pub max_age_secs: u64,
    pub max_points: usize,
}

impl RetentionPolicy {
    pub fn new(max_age_secs: u64, max_points: usize) -> Self {
        RetentionPolicy {
            max_age_secs,
            max_points,
        }
    }
}
