#[derive(Clone, Debug)]
pub struct TimeRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug)]
pub enum Aggregation {
    Sum,
    Avg,
    Min,
    Max,
    Count,
    Last,
}

#[derive(Clone, Debug)]
pub struct QueryResult {
    pub name: String,
    pub range: TimeRange,
    pub aggregation: Aggregation,
    pub value: f64,
    pub points: Vec<super::series::DataPoint>,
}
