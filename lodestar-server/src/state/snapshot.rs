use std::collections::BTreeSet;

use futures::future::join_all;
use lodestar_proto::{ChannelView, Snapshot};

use super::AppState;
use super::host_view::host_view_with;
use super::service_view::service_views;

/// One view per channel used by a managed host, sorted by name.
pub async fn channels(state: &AppState) -> Vec<ChannelView> {
    let names: BTreeSet<&str> = state
        .hosts
        .iter()
        .map(|host| host.channel.as_str())
        .collect();
    join_all(names.into_iter().map(|name| async move {
        let head = state.mirror.head(name).await;
        let head_subject = match &head {
            Some(commit) => state.mirror.subject(commit).await,
            None => None,
        };
        ChannelView {
            name: name.to_string(),
            head,
            head_subject,
        }
    }))
    .await
}

pub async fn snapshot(state: &AppState, viewer: String) -> Snapshot {
    let peers = state.online.peers().await;
    let hosts = join_all(
        state
            .hosts
            .iter()
            .map(|host| host_view_with(state, host, &peers)),
    )
    .await;
    let services = service_views(state, &peers);
    Snapshot {
        hosts,
        channels: channels(state).await,
        promote_candidate: state.mirror.promote_candidate().await,
        promote: state.mirror.promote_channels().cloned(),
        viewer,
        demo: state.demo,
        services,
    }
}
