// /* ../NauTrader_Helm/eryx/crates/eryx-python/src/telemetry.rs */
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::util::SubscriberInitExt;

static PROVIDER: OnceLock<Mutex<Option<SdkTracerProvider>>> = OnceLock::new();

fn provider_slot() -> &'static Mutex<Option<SdkTracerProvider>> {
    PROVIDER.get_or_init(|| Mutex::new(None))
}

#[pyfunction]
pub(crate) fn init_telemetry(
    endpoint: &str,
    headers: HashMap<String, String>,
    service_name: &str,
    environment: &str,
) -> PyResult<bool> {
    let mut slot = provider_slot()
        .lock()
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    if slot.is_some() {
        return Ok(false);
    }
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(format!("{}/v1/traces", endpoint.trim_end_matches('/')))
        .with_headers(headers)
        .build()
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    let resource = Resource::builder()
        .with_service_name(service_name.to_owned())
        .with_attribute(KeyValue::new(
            "deployment.environment.name",
            environment.to_owned(),
        ))
        .build();
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    let tracer = provider.tracer("eryx");
    let layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(filter_fn(|metadata| {
            metadata.target().starts_with("eryx::")
        }));
    if let Err(error) = tracing_subscriber::registry()
        .with(layer)
        .try_init()
    {
        let _ = provider.shutdown();
        return Err(PyRuntimeError::new_err(format!(
            "Failed to install ERYX tracing subscriber: {error}"
        )));
    }
    *slot = Some(provider);
    Ok(true)
}

#[pyfunction]
pub(crate) fn shutdown_telemetry() -> PyResult<()> {
    let provider = provider_slot()
        .lock()
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?
        .take();
    if let Some(provider) = provider {
        provider
            .force_flush()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        provider
            .shutdown()
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
    }
    Ok(())
}
