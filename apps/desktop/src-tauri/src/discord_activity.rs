use std::{
    sync::{
        Arc, Mutex,
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
const LARGE_IMAGE_ASSET_KEY: &str = "trace-activity";
const UPDATE_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(crate) struct SharedDiscordActivity {
    enabled: Arc<AtomicBool>,
    review: Arc<Mutex<Option<ReviewActivity>>>,
}

impl SharedDiscordActivity {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn set_review(&self, review: Option<ReviewActivity>) {
        if let Ok(mut current) = self.review.lock() {
            *current = review;
        }
    }

    fn review(&self) -> Option<ReviewActivity> {
        self.review.lock().ok()?.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReviewActivity {
    pub(crate) kind: ReviewKind,
    pub(crate) simulator: Option<String>,
    pub(crate) track: Option<String>,
    pub(crate) car: Option<String>,
    pub(crate) session_type: Option<String>,
    pub(crate) lap_index: Option<u32>,
    pub(crate) started_at_unix: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReviewKind {
    Sessions,
    Session,
    Lap,
    Comparison,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DesiredActivity {
    Driving {
        session: PresenceSession,
        spectator_url: Option<String>,
    },
    Reviewing(ReviewActivity),
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
            .then(|| desired_activity(setting, capture_status, live_broadcast))
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
    setting: &SharedDiscordActivity,
    capture_status: &SharedCaptureStatus,
    live_broadcast: &SharedLiveBroadcast,
) -> Option<DesiredActivity> {
    let session = capture_status
        .lock()
        .ok()
        .and_then(|status| status.presence_session.clone());
    if let Some(session) = session {
        let spectator_url = live_broadcast.snapshot().ok().and_then(|status| {
            matches!(status.phase, LiveBroadcastPhase::Live)
                .then_some(status.spectator_url)
                .flatten()
                .filter(|url| is_public_spectator_url(url))
        });
        return Some(DesiredActivity::Driving {
            session,
            spectator_url,
        });
    }
    setting.review().map(DesiredActivity::Reviewing)
}

fn publish(
    client: &mut DiscordIpcClient,
    desired: &DesiredActivity,
) -> Result<(), discord_rich_presence::error::Error> {
    let (details, state, started_at, spectator_url) = match desired {
        DesiredActivity::Driving {
            session,
            spectator_url,
        } => (
            bounded_activity_text(&format!(
                "{} · {}",
                display_session_type(&session.session_type),
                session.track
            )),
            bounded_activity_text(&format!(
                "{} · {}",
                simulator_name(&session.simulator),
                session.car
            )),
            session.started_at_unix,
            spectator_url.as_deref(),
        ),
        DesiredActivity::Reviewing(review) => (
            review_details(review),
            review_state(review),
            review.started_at_unix,
            None,
        ),
    };
    let mut value = activity::Activity::new()
        .details(details)
        .state(state)
        .assets(
            activity::Assets::new()
                .large_image(LARGE_IMAGE_ASSET_KEY)
                .large_text("TRACE · Sim racing telemetry"),
        )
        .timestamps(activity::Timestamps::new().start(started_at));
    if let Some(url) = spectator_url {
        value = value.buttons(vec![activity::Button::new("Watch Live", url)]);
    }
    client.set_activity(value)
}

fn review_details(review: &ReviewActivity) -> String {
    let value = match review.kind {
        ReviewKind::Sessions => "Browsing recorded sessions".to_owned(),
        ReviewKind::Session => review.track.as_ref().map_or_else(
            || "Reviewing a recorded session".to_owned(),
            |track| {
                review.session_type.as_ref().map_or_else(
                    || format!("Reviewing session · {track}"),
                    |session_type| {
                        format!("Reviewing {} · {track}", display_session_type(session_type))
                    },
                )
            },
        ),
        ReviewKind::Lap => match (review.lap_index, review.track.as_deref()) {
            (Some(lap), Some(track)) => format!("Reviewing lap {lap} · {track}"),
            (Some(lap), None) => format!("Reviewing lap {lap}"),
            _ => "Reviewing lap telemetry".to_owned(),
        },
        ReviewKind::Comparison => review.track.as_ref().map_or_else(
            || "Comparing recorded laps".to_owned(),
            |track| format!("Comparing laps · {track}"),
        ),
    };
    bounded_activity_text(&value)
}

fn review_state(review: &ReviewActivity) -> String {
    let simulator = review
        .simulator
        .as_deref()
        .map(simulator_name)
        .unwrap_or("TRACE");
    bounded_activity_text(&review.car.as_ref().map_or_else(
        || simulator.to_owned(),
        |car| format!("{simulator} · {car}"),
    ))
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

    #[test]
    fn review_activity_is_human_readable_without_private_identity() {
        let review = ReviewActivity {
            kind: ReviewKind::Lap,
            simulator: Some("assetto-corsa".into()),
            track: Some("Zandvoort".into()),
            car: Some("Mazda MX-5 Cup".into()),
            session_type: Some("hotlap".into()),
            lap_index: Some(4),
            started_at_unix: 1,
        };
        assert_eq!(review_details(&review), "Reviewing lap 4 · Zandvoort");
        assert_eq!(review_state(&review), "Assetto Corsa · Mazda MX-5 Cup");
    }
}
