mod bytes_processed;
mod errors;
mod events_processed;
mod host;
mod uptime;

use crate::event::{Event, Metric, MetricValue};
use crate::metrics::{capture_metrics, get_controller, Controller};
use async_graphql::{validators::IntRange, Interface, Object, Subscription};
use async_stream::stream;
use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::stream::{Stream, StreamExt};
use tokio::time::Duration;

pub use bytes_processed::{ComponentProcessedBytesTotal, ProcessedBytesTotal};
pub use errors::{ComponentErrorsTotal, ErrorsTotal};
pub use events_processed::{ComponentEventsProcessedTotal, EventsProcessedTotal};
pub use host::HostMetrics;
use nom::lib::std::collections::BTreeMap;
pub use uptime::Uptime;

lazy_static! {
    static ref GLOBAL_CONTROLLER: Arc<&'static Controller> =
        Arc::new(get_controller().expect("Metrics system not initialized. Please report."));
}

#[derive(Interface)]
#[graphql(field(name = "timestamp", type = "Option<DateTime<Utc>>"))]
pub enum MetricType {
    Uptime(Uptime),
    EventsProcessedTotal(EventsProcessedTotal),
    ProcessedBytesTotal(ProcessedBytesTotal),
}

#[derive(Default)]
pub struct MetricsQuery;

#[Object]
impl MetricsQuery {
    /// Vector host metrics
    async fn host_metrics(&self) -> HostMetrics {
        HostMetrics::new()
    }
}

#[derive(Default)]
pub struct MetricsSubscription;

#[Subscription]
impl MetricsSubscription {
    /// Metrics for how long the Vector instance has been running
    async fn uptime(
        &self,
        #[graphql(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = Uptime> {
        get_metrics(interval).filter_map(|m| match m.name.as_str() {
            "uptime_seconds" => Some(Uptime::new(m)),
            _ => None,
        })
    }

    /// Events processed metrics
    async fn events_processed_total(
        &self,
        #[arg(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = EventsProcessedTotal> {
        get_metrics(interval).filter_map(|m| match m.name.as_str() {
            "events_processed_total" => Some(EventsProcessedTotal::new(m)),
            _ => None,
        })
    }

    /// Component events processed metrics. Streams new data as the metric increases
    async fn component_events_processed_total(
        &self,
        #[arg(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = ComponentEventsProcessedTotal> {
        component_counter_metrics(interval)
            .filter(|m| m.name == "events_processed_total")
            .map(ComponentEventsProcessedTotal::new)
    }

    /// Bytes processed metrics
    async fn processed_bytes_total(
        &self,
        #[graphql(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = ProcessedBytesTotal> {
        get_metrics(interval).filter_map(|m| match m.name.as_str() {
            "processed_bytes_total" => Some(ProcessedBytesTotal::new(m)),
            _ => None,
        })
    }

    /// Component events processed metrics. Streams new data as the metric increases
    async fn component_processed_bytes_total(
        &self,
        #[arg(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = ComponentProcessedBytesTotal> {
        component_counter_metrics(interval)
            .filter(|m| m.name == "processed_bytes_total")
            .map(ComponentProcessedBytesTotal::new)
    }

    /// Total error metrics
    async fn errors_total(
        &self,
        #[arg(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = ErrorsTotal> {
        get_metrics(interval)
            .filter(|m| m.name.ends_with("_errors_total"))
            .map(ErrorsTotal::new)
    }

    /// Component errors metrics. Streams new data as the metric increases
    async fn component_errors_total(
        &self,
        #[arg(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = ComponentErrorsTotal> {
        component_counter_metrics(interval)
            .filter(|m| m.name.ends_with("_errors_total"))
            .map(ComponentErrorsTotal::new)
    }

    /// All metrics
    async fn metrics(
        &self,
        #[graphql(default = 1000, validator(IntRange(min = "100", max = "60_000")))] interval: i32,
    ) -> impl Stream<Item = MetricType> {
        get_metrics(interval).filter_map(|m| match m.name.as_str() {
            "uptime_seconds" => Some(MetricType::Uptime(m.into())),
            "events_processed_total" => Some(MetricType::EventsProcessedTotal(m.into())),
            "processed_bytes_total" => Some(MetricType::ProcessedBytesTotal(m.into())),
            _ => None,
        })
    }
}

/// Returns a stream of `Metric`s, collected at the provided millisecond interval
fn get_metrics(interval: i32) -> impl Stream<Item = Metric> {
    let controller = get_controller().unwrap();
    let mut interval = tokio::time::interval(Duration::from_millis(interval as u64));

    stream! {
        loop {
            interval.tick().await;
            for ev in capture_metrics(&controller) {
                if let Event::Metric(m) = ev {
                    yield m;
                }
            }
        }
    }
}

/// Get the events processed by component name
pub fn component_events_processed_total(component_name: String) -> Option<EventsProcessedTotal> {
    let key = String::from("component_name");

    capture_metrics(&GLOBAL_CONTROLLER)
        .find(|ev| match ev {
            Event::Metric(m)
                if m.name.as_str().eq("events_processed_total")
                    && m.tag_matches(&key, &component_name) =>
            {
                true
            }
            _ => false,
        })
        .map(|ev| EventsProcessedTotal::new(ev.into_metric()))
}

/// Returns a stream of metrics, where `metric_name` matches the name of the metric
/// (e.g. "events_processed"), and the value is derived from `MetricValue::Counter`. Uses a
/// local cache to match against the `component_name` of a metric, to return results only when
/// the value of a current iteration is greater than the previous. This is useful for the client
/// to be notified as metrics increase without returning 'empty' or identical results.
pub fn component_counter_metrics(interval: i32) -> impl Stream<Item = Metric> {
    let mut cache = BTreeMap::new();

    get_metrics(interval).filter_map(move |m| match m.tag_value("component_name") {
        Some(name) => match m.value {
            MetricValue::Counter { value } if cache.insert(name, value).unwrap_or(0.00) < value => {
                Some(m)
            }
            _ => None,
        },
        _ => None,
    })
}