#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub score: f64,
}

fn less(a: &Entry, b: &Entry) -> bool {
    if a.score != b.score {
        return a.score < b.score;
    }
    a.name < b.name
}

pub struct ZSet {
    by_name: std::collections::HashMap<String, f64>,
    sorted: Vec<Entry>,
}

impl ZSet {
    pub fn new() -> Self {
        ZSet {
            by_name: std::collections::HashMap::new(),
            sorted: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.sorted.len()
    }

    pub fn add(&mut self, name: String, score: f64) -> bool {
        if let Some(&old) = self.by_name.get(&name) {
            if (old - score).abs() < f64::EPSILON {
                return false;
            }
            self.remove(&name);
        }
        self.by_name.insert(name.clone(), score);
        let e = Entry { name, score };
        let i = match self.sorted.binary_search_by(|probe| {
            if less(probe, &e) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }) {
            Ok(i) | Err(i) => i,
        };
        self.sorted.insert(i, e);
        true
    }

    pub fn remove(&mut self, name: &str) -> bool {
        if let Some(&score) = self.by_name.get(name) {
            self.by_name.remove(name);
            let key = Entry {
                name: name.to_string(),
                score,
            };
            if let Ok(i) = self.sorted.binary_search_by(|probe| {
                if less(probe, &key) {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            }) {
                if i < self.sorted.len() && self.sorted[i].name == name {
                    self.sorted.remove(i);
                }
            }
            true
        } else {
            false
        }
    }

    pub fn score(&self, name: &str) -> Option<f64> {
        self.by_name.get(name).copied()
    }

    pub fn query(&self, min_score: f64, min_name: &str, offset: i64, limit: i64) -> Vec<Entry> {
        if limit <= 0 {
            return Vec::new();
        }
        let key = Entry {
            name: min_name.to_string(),
            score: min_score,
        };
        let pos = match self.sorted.binary_search_by(|probe| {
            if less(probe, &key) {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }) {
            Ok(i) | Err(i) => i,
        };
        let pos = pos.saturating_add_signed(offset as isize).max(0);
        let end = (pos + limit as usize).min(self.sorted.len());
        if pos >= end {
            return Vec::new();
        }
        self.sorted[pos..end].to_vec()
    }
}
