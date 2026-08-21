pub mod query;
pub mod retention;
pub mod series;

pub use query::{Aggregation, QueryResult, TimeRange};
pub use retention::RetentionPolicy;
pub use series::TimeSeriesDB;

// Note: TSDB is integrated via handler commands (TS.ADD, TS.RANGE, TS.INFO, TS.LIST)
// The TimeSeriesDB struct can be used for more advanced in-memory queries
