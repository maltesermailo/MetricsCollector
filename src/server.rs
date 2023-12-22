use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server,
};
use prometheus_client::{encoding::text::encode, registry::Registry};
use std::{future::Future, io, net::{IpAddr, Ipv4Addr, SocketAddr}, pin::Pin, sync::Arc, thread};
use std::ops::Deref;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64};
use std::time::Duration;
use prometheus_client::metrics::gauge::Gauge;
use sysinfo::{CpuExt, System, SystemExt};

use tokio::signal::unix::{signal, SignalKind};

pub async fn start() {
    //let request_counter: Counter<u64> = Default::default();
    let cpu_usage: Arc<Gauge<f64, AtomicU64>> = Arc::new(Gauge::<f64, AtomicU64>::default());
    let memory_usage: Arc<Gauge<f64, AtomicU64>> = Arc::new(Gauge::<f64, AtomicU64>::default());
    let memory_total: Arc<Gauge<i64, AtomicI64>> = Arc::new(Gauge::<i64, AtomicI64>::default());
    let memory_used: Arc<Gauge<i64, AtomicI64>> = Arc::new(Gauge::<i64, AtomicI64>::default());
    let swap_usage: Arc<Gauge<f64, AtomicU64>> = Arc::new(Gauge::<f64, AtomicU64>::default());
    let swap_total: Arc<Gauge<i64, AtomicI64>> = Arc::new(Gauge::<i64, AtomicI64>::default());
    let swap_used: Arc<Gauge<i64, AtomicI64>> = Arc::new(Gauge::<i64, AtomicI64>::default());

    let mut registry = <Registry>::with_prefix("node_status");

    registry.register(
        "CPU Usage",
        "CPU Usage of the Node",
        cpu_usage.deref().clone(),
    );

    registry.register("Memory Usage",
                      "The Memory Usage probably in percent",
                      memory_usage.deref().clone()
    );
    registry.register("Swap Usage",
                      "The Swap Usage probably in percent",
                      swap_usage.deref().clone()
    );
    registry.register("Memory Used",
                      "The Memory Usage probably in MB",
                      memory_used.deref().clone()
    );
    registry.register("Memory Total",
                      "The Memory Total probably in MB",
                      memory_total.deref().clone()
    );
    registry.register("Swap Used",
                      "The Swap Usage probably in MB",
                      swap_used.deref().clone()
    );
    registry.register("Swap Total",
                      "The Swap Total probably in MB",
                      swap_total.deref().clone()
    );

    //Create update thread
    thread::spawn(move || {
        let mut sys = System::new_all();

        thread::sleep(Duration::from_secs(1));

        //Refresh system statistics
        sys.refresh_all();

        let usage_percent: f64 = (sys.used_memory() as f64 / sys.total_memory() as f64);
        let swap_percent: f64 = (sys.used_swap() as f64 / sys.total_swap() as f64);

        cpu_usage.set(sys.global_cpu_info().cpu_usage() as f64);
        memory_usage.set(usage_percent);
        memory_total.set(sys.total_memory() as i64);
        memory_used.set(sys.used_memory() as i64);
        swap_usage.set(swap_percent);
        swap_total.set(sys.total_swap() as i64);
        swap_used.set(sys.used_swap() as i64);
    });

    // Spawn a server to serve the OpenMetrics endpoint.
    let metrics_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8001);
    start_metrics_server(metrics_addr, registry).await
}

/// Start a HTTP server to report metrics.
pub async fn start_metrics_server(metrics_addr: SocketAddr, registry: Registry) {
    let mut shutdown_stream = signal(SignalKind::terminate()).unwrap();

    eprintln!("Starting metrics server on {metrics_addr}");

    let registry = Arc::new(registry);
    Server::bind(&metrics_addr)
        .serve(make_service_fn(move |_conn| {
            let registry = registry.clone();
            async move {
                let handler = make_handler(registry);
                Ok::<_, io::Error>(service_fn(handler))
            }
        }))
        .with_graceful_shutdown(async move {
            shutdown_stream.recv().await;
        })
        .await
        .unwrap();
}

/// This function returns a HTTP handler (i.e. another function)
pub fn make_handler(
    registry: Arc<Registry>,
) -> impl Fn(Request<Body>) -> Pin<Box<dyn Future<Output = io::Result<Response<Body>>> + Send>> {
    // This closure accepts a request and responds with the OpenMetrics encoding of our metrics.
    move |_req: Request<Body>| {
        let reg = registry.clone();
        Box::pin(async move {
            let mut buf = String::new();
            encode(&mut buf, &reg.clone())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                .map(|_| {
                    let body = Body::from(buf);
                    Response::builder()
                        .header(
                            hyper::header::CONTENT_TYPE,
                            "application/openmetrics-text; version=1.0.0; charset=utf-8",
                        )
                        .body(body)
                        .unwrap()
                })
        })
    }
}
