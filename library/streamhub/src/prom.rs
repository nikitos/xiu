use lazy_static::lazy_static;

use prometheus::{Gauge};
use prometheus::{register_gauge};

lazy_static! {
    pub static ref STREAMS_TOTAL: Gauge = register_gauge!(
        "streams_total",
        "Total number of streams pushed."
    )
    .unwrap();
}