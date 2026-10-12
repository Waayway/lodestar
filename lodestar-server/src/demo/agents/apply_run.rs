//! The simulated apply: a realistic log, step by step, then the scripted outcome.

use std::sync::Arc;

use lodestar_proto::{ActivationEvent, ApplyState, SkipReason};

use super::store_paths::{store_hash, system_path};
use super::{Script, SimAgent};
use crate::clock::now;

impl SimAgent {
    /// Runs one apply: the log appears step by step, then the record gets its outcome.
    pub(super) async fn run_apply(self: Arc<Self>, id: String, commit: String) {
        let short = &commit[..7];
        let hash = |what: &str| store_hash(&format!("{id}{what}"));
        let (head, channel) = {
            let inner = self.inner.lock().unwrap();
            (
                inner.checkout.head.clone().unwrap_or_default(),
                inner.checkout.branch.clone().unwrap_or_default(),
            )
        };
        let old = &head[..head.len().min(7)];

        let step = |secs: f64, text: String| {
            let agent = self.clone();
            let id = id.clone();
            async move {
                tokio::time::sleep(agent.scaled(secs)).await;
                agent.append_log(&id, &text);
            }
        };

        step(0.2, "==> git fetch".into()).await;
        step(
            1.0,
            format!(
                "From https://example.com/infra.git\n   {old}..{short}  {channel:<10} -> origin/{channel}"
            ),
        )
        .await;
        self.inner.lock().unwrap().checkout.last_fetch_at = Some(now());
        step(0.3, "==> checks".into()).await;
        if self.script == Script::Dirty {
            step(0.3, "==> skipped: Dirty".into()).await;
            self.finish(&id, |record| {
                record.state = ApplyState::Skipped;
                record.skip_reason = Some(SkipReason::Dirty);
                record.message = Some("skipped: Dirty".into());
            });
            return;
        }
        step(
            0.3,
            format!("==> git merge --ff-only\nUpdating {old}..{short}\nFast-forward"),
        )
        .await;
        self.inner.lock().unwrap().checkout.head = Some(commit.clone());

        let (flake_attr, system_drv) = if self.os == "darwin" {
            (
                format!("darwinConfigurations.{}.system", self.name),
                format!("darwin-system-25.11.{short}"),
            )
        } else {
            (
                format!(
                    "nixosConfigurations.{}.config.system.build.toplevel",
                    self.name
                ),
                format!("nixos-system-{}-25.11.20261010.{short}", self.name),
            )
        };
        step(
            0.4,
            format!(
                "==> nix build\nevaluating {flake_attr}\nthese 4 derivations will be built:\n  /nix/store/{}-home-manager-files.drv\n  /nix/store/{}-home-manager-generation.drv\n  /nix/store/{}-etc.drv\n  /nix/store/{}-{system_drv}.drv",
                hash("hmf"),
                hash("hmg"),
                hash("etc"),
                hash("sys"),
            ),
        )
        .await;
        step(
            1.4,
            format!(
                "building '/nix/store/{}-home-manager-files.drv'...",
                hash("hmf")
            ),
        )
        .await;
        let fail = self.script == Script::FailFirst && !self.inner.lock().unwrap().failed_once;
        step(
            1.6,
            format!(
                "building '/nix/store/{}-home-manager-generation.drv'...",
                hash("hmg")
            ),
        )
        .await;
        if fail {
            step(
                1.2,
                format!(
                    "error: builder for '/nix/store/{}-home-manager-generation.drv' failed with exit code 1\n==> nix build failed (exit status: 1)",
                    hash("hmg")
                ),
            )
            .await;
            self.inner.lock().unwrap().failed_once = true;
            self.finish(&id, |record| {
                record.state = ApplyState::Failed;
                record.exit_code = Some(1);
                record.message = Some("nix build failed (exit status: 1)".into());
            });
            return;
        }
        step(
            1.2,
            format!("building '/nix/store/{}-etc.drv'...", hash("etc")),
        )
        .await;
        step(
            1.4,
            format!("building '/nix/store/{}-{system_drv}.drv'...", hash("sys")),
        )
        .await;
        let store_path = system_path(&self.name, &self.os, &commit);
        step(0.6, store_path.clone()).await;
        step(0.3, "==> nix-env --set".into()).await;
        let activation = if self.os == "darwin" {
            "==> activate\nsetting up groups...\nsetting up users...\nsetting up /Applications/Nix Apps...\nsetting up pam...\nactivating home-manager configuration for demo"
        } else {
            "==> switch-to-configuration\nactivating the configuration...\nsetting up /etc...\nreloading user units for demo...\nrestarting sysinit-reactivation.target\nthe following new units were started: nix-gc.timer"
        };
        step(0.8, activation.into()).await;

        let time = now();
        let mut inner = self.inner.lock().unwrap();
        inner.generation += 1;
        let event = ActivationEvent {
            line: inner.activations.len() as u64,
            time,
            store_path,
            revision: commit.clone(),
            generation: Some(inner.generation),
            trigger: format!("apply:{id}"),
            sudo_user: None,
        };
        inner.activations.push(event);
        inner.running = commit;
        drop(inner);
        self.finish(&id, |record| {
            record.state = ApplyState::Ok;
            record.exit_code = Some(0);
        });
    }
}
