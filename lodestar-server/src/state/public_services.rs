use lodestar_proto::{PublicService, PublicServiceState};

use super::AppState;

/// What an anonymous visitor may see: one entry per link, sorted by `(group, title)`. A link
/// without a check result is `Unknown`.
pub fn public_services(state: &AppState) -> Vec<PublicService> {
    let checks = state.checks.lock().unwrap();
    let mut services: Vec<PublicService> = state
        .links
        .iter()
        .map(|link| {
            let check = checks.get(&link.id);
            PublicService {
                title: link.title.clone(),
                group: link.group.clone(),
                description: link.description.clone(),
                icon: link.icon.clone(),
                state: check.map_or(PublicServiceState::Unknown, |check| {
                    PublicServiceState::from_check(&check.state)
                }),
                since: check.and_then(|check| check.since),
            }
        })
        .collect();
    services.sort_by(|left, right| (&left.group, &left.title).cmp(&(&right.group, &right.title)));
    services
}
