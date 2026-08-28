use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use discord_rich_presence::{DiscordIpc, DiscordIpcClient, activity};

use crate::{
    capture::{PresenceSession, SharedCaptureStatus},
    live_broadcast::{LiveBroadcastPhase, SharedLiveBroadcast},
};

const APPLICATION_ID: &str = "1540877666167689216";
const UPDATE_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(crate) struct SharedDiscordActivity {
    enabled: Arc<AtomicBool>,
}

impl SharedDiscordActivity {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DesiredActivity {
    session: PresenceSession,
    spectator_url: Option<String>,
}

pub(crate) fn spawn(
    setting: SharedDiscordActivity,
    capture_status: SharedCaptureStatus,
    live_broadcast: SharedLiveBroadcast,
) {
    thread::Builder::new()
        .name("trace-discord-activity".into())
        .spawn(move || run(&setting, &capture_status, &live_broadcast))
        .expect("failed to start Discord activity worker");
}

fn run(
    setting: &SharedDiscordActivity,
    capture_status: &SharedCaptureStatus,
    live_broadcast: &SharedLiveBroadcast,
) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut published: Option<DesiredActivity> = None;
    let mut refresh_ticks = 0_u8;

    loop {
        refresh_ticks = refresh_ticks.saturating_add(1);
        if refresh_ticks >= 15 {
            published = None;
            refresh_ticks = 0;
        }
        let desired = setting
            .enabled()
            .then(|| desired_activity(capture_status, live_broadcast))
            .flatten();
        if desired != published {
            if let Some(activity) = desired.as_ref() {
                if client.is_none() {
                    let mut next_client = DiscordIpcClient::new(APPLICATION_ID);
                    client = next_client.connect().ok().map(|()| next_client);
                }
                let result = client.as_mut().map(|client| publish(client, activity));
                if matches!(result, Some(Ok(()))) {
                    published = desired;
                } else {
                    client = None;
                }
            } else {
                if let Some(active_client) = client.as_mut() {
                    let _ = active_client.clear_activity();
                }
                published = None;
            }
        }
        thread::sleep(UPDATE_INTERVAL);
    }
}

fn desired_activity(
    capture_status: &SharedCaptureStatus,
    live_broadcast: &SharedLiveBroadcast,
) -> Option<DesiredActivity> {
    let session = capture_status.lock().ok()?.presence_session.clone()?;
    let spectator_url = live_broadcast.snapshot().ok().and_then(|status| {
        matches!(status.phase, LiveBroadcastPhase::Live)
            .then_some(status.spectator_url)
            .flatten()
            .filter(|url| is_public_spectator_url(url))
    });
    Some(DesiredActivity {
        session,
        spectator_url,
    })
}

fn publish(
    client: &mut DiscordIpcClient,
    desired: &DesiredActivity,
) -> Result<(), discord_rich_presence::error::Error> {
    let details = bounded_activity_text(&format!(
        "{} · {}",
        display_session_type(&desired.session.session_type),
        desired.session.track
    ));
    let state = bounded_activity_text(&format!(
        "{} · {}",
        simulator_name(&desired.session.simulator),
        desired.session.car
    ));
    let mut value = activity::Activity::new()
        .details(details)
        .state(state)
        .timestamps(activity::Timestamps::new().start(desired.session.started_at_unix));
    if let Some(url) = desired.spectator_url.as_deref() {
        value = value.buttons(vec![activity::Button::new("Watch Live", url)]);
    }
    client.set_activity(value)
}

fn bounded_activity_text(value: &str) -> String {
    value.chars().take(128).collect()
}

fn is_public_spectator_url(url: &str) -> bool {
    url.starts_with("https://")
}

fn display_session_type(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn simulator_name(value: &str) -> &str {
    match value {
        "assetto-corsa" => "Assetto Corsa",
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_spectator_urls_are_publishable() {
        assert!(is_public_spectator_url("https://live.simtrace.run/s/abc"));
        assert!(!is_public_spectator_url("http://127.0.0.1:9876/s/abc"));
        assert!(!is_public_spectator_url("http://localhost:9876/s/abc"));
    }

    #[test]
    fn session_types_are_human_readable() {
        assert_eq!(display_session_type("time attack"), "Time Attack");
        assert_eq!(display_session_type("hotlap"), "Hotlap");
    }

    #[test]
    fn activity_text_respects_discords_character_limit() {
        let value = "🏁".repeat(200);
        assert_eq!(bounded_activity_text(&value).chars().count(), 128);
    }
}
