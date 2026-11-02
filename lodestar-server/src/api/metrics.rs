//! `GET /metrics` on the dedicated metrics listener.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};

use crate::builds::build_metrics;
use crate::state::AppState;
use crate::updates::metrics::{MetricsInput, render};

pub async fn get_metrics(State(state): State<AppState>) -> Response {
    let running: BTreeMap<_, _> = state
        .versions
        .lock()
        .unwrap()
        .iter()
        .map(|(host, versions)| (host.clone(), versions.clone()))
        .collect();
    let latest = state
        .latest
        .lock()
        .unwrap()
        .iter()
        .filter(|(software, _)| state.config.updates.software.contains_key(*software))
        .map(|(software, latest)| (software.clone(), latest.clone()))
        .collect();
    let last_runs = state.updates.last.lock().unwrap().clone();
    let builds = build_metrics(&state).await;
    let body = render(&MetricsInput {
        running,
        latest,
        last_runs,
        builds,
    });
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; version=0.0.4"),
        )],
        body,
    )
        .into_response()
}
