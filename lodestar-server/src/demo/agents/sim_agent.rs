use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use lodestar_proto::{ActivationEvent, AgentStatus, ApplyRecord, CheckoutStatus};

use super::store_paths::system_path;
use super::{HostSetup, Script};
use crate::clock::now;

const AGENT_VERSION: &str = "0.1.0-demo";

/// One simulated apply: its record and the log so far.
pub(super) struct Apply {
    pub(super) record: ApplyRecord,
    pub(super) log: String,
}

/// The mutable part of a simulated agent.
pub(super) struct Inner {
    pub(super) running: String,
    pub(super) checkout: CheckoutStatus,
    pub(super) applies: VecDeque<Apply>,
    pub(super) activations: Vec<ActivationEvent>,
    pub(super) generation: u64,
    pub(super) failed_once: bool,
}

pub struct SimAgent {
    pub(super) name: String,
    pub(super) os: String,
    pub(super) script: Script,
    speed: f64,
    pub(super) inner: Mutex<Inner>,
}

impl SimAgent {
    pub fn new(setup: HostSetup, speed: f64) -> SimAgent {
        let first = ActivationEvent {
            line: 0,
            time: setup.activated_at,
            store_path: system_path(&setup.name, &setup.os, &setup.running),
            revision: setup.running.clone(),
            generation: Some(setup.generation),
            trigger: "manual".into(),
            sudo_user: Some(setup.user.clone()),
        };
        let checkout = CheckoutStatus {
            branch: Some(setup.channel.clone()),
            head: Some(setup.running.clone()),
            dirty: setup.script == Script::Dirty,
            untracked: if setup.script == Script::Dirty { 2 } else { 0 },
            ahead: Some(0),
            last_fetch_at: Some(now() - 4 * 60),
            flake_path: Some(if setup.os == "darwin" {
                format!("/Users/{}/.flake", setup.user)
            } else {
                format!("/home/{}/.flake", setup.user)
            }),
        };
        SimAgent {
            name: setup.name,
            os: setup.os,
            script: setup.script,
            speed,
            inner: Mutex::new(Inner {
                running: setup.running,
                checkout,
                applies: VecDeque::new(),
                activations: vec![first],
                generation: setup.generation,
                failed_once: false,
            }),
        }
    }

    pub(super) fn status(&self) -> AgentStatus {
        let inner = self.inner.lock().unwrap();
        AgentStatus {
            host: self.name.clone(),
            os: self.os.clone(),
            agent_version: AGENT_VERSION.into(),
            running: inner.running.clone(),
            checkout: inner.checkout.clone(),
            apply: inner.applies.back().map(|apply| apply.record.clone()),
        }
    }

    pub(super) fn append_log(&self, apply_id: &str, text: &str) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(apply) = inner
            .applies
            .iter_mut()
            .find(|apply| apply.record.apply_id == apply_id)
        {
            apply.log.push_str(text);
            if !text.ends_with('\n') {
                apply.log.push('\n');
            }
        }
    }

    pub(super) fn scaled(&self, secs: f64) -> Duration {
        Duration::from_secs_f64(secs / self.speed)
    }

    pub(super) fn finish(&self, id: &str, outcome: impl FnOnce(&mut ApplyRecord)) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(apply) = inner
            .applies
            .iter_mut()
            .find(|apply| apply.record.apply_id == id)
        {
            outcome(&mut apply.record);
            apply.record.finished_at = Some(now());
        }
    }
}
