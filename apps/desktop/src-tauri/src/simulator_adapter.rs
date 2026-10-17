use trace_ac::AcAdapter;
use trace_adapter::{AdapterError, AdapterEvent, AdapterIdentity, SimulatorAdapter};
use trace_lmu::LmuAdapter;

use crate::capture::SharedCaptureStatus;

enum SelectedAdapter {
    Auto(Box<AutoAdapter>),
    AssettoCorsa(AcAdapter),
    LeMansUltimate(LmuAdapter),
}

impl SelectedAdapter {
    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match self {
            Self::Auto(adapter) => adapter.poll(),
            Self::AssettoCorsa(adapter) => adapter.poll(),
            Self::LeMansUltimate(adapter) => adapter.poll(),
        }
    }
}

#[derive(Clone, Copy)]
enum ActiveSimulator {
    AssettoCorsa,
    LeMansUltimate,
}

struct AutoAdapter<A = AcAdapter, L = LmuAdapter> {
    assetto_corsa: A,
    le_mans_ultimate: L,
    active: Option<ActiveSimulator>,
}

impl AutoAdapter {
    fn new(steering_lock_degrees: Option<f32>) -> Self {
        Self {
            assetto_corsa: AcAdapter::with_steering_lock_degrees(steering_lock_degrees),
            le_mans_ultimate: LmuAdapter::new(),
            active: None,
        }
    }
}

impl<A: SimulatorAdapter, L: SimulatorAdapter> AutoAdapter<A, L> {
    #[cfg(test)]
    fn with_adapters(assetto_corsa: A, le_mans_ultimate: L) -> Self {
        Self {
            assetto_corsa,
            le_mans_ultimate,
            active: None,
        }
    }

    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match self.active {
            Some(ActiveSimulator::AssettoCorsa) => {
                let result = self.assetto_corsa.poll();
                self.clear_active_if_disconnected(&result);
                result
            }
            Some(ActiveSimulator::LeMansUltimate) => {
                let result = self.le_mans_ultimate.poll();
                self.clear_active_if_disconnected(&result);
                result
            }
            None => self.detect_active(),
        }
    }

    fn detect_active(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        let assetto_corsa = self.assetto_corsa.poll();
        if assetto_corsa
            .as_ref()
            .is_ok_and(|events| !events.is_empty())
        {
            self.active = Some(ActiveSimulator::AssettoCorsa);
            return assetto_corsa;
        }

        let le_mans_ultimate = self.le_mans_ultimate.poll();
        if le_mans_ultimate
            .as_ref()
            .is_ok_and(|events| !events.is_empty())
        {
            self.active = Some(ActiveSimulator::LeMansUltimate);
            return le_mans_ultimate;
        }

        assetto_corsa.and(le_mans_ultimate)
    }

    fn clear_active_if_disconnected(&mut self, result: &Result<Vec<AdapterEvent>, AdapterError>) {
        if result.is_err()
            || result.as_ref().is_ok_and(|events| {
                events
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::Disconnected(_)))
            })
        {
            self.active = None;
        }
    }
}

/// Routes the single capture worker to the simulator selected in the desktop UI.
pub struct SelectedSimulatorAdapter {
    identity: AdapterIdentity,
    status: SharedCaptureStatus,
    selected_id: String,
    steering_lock_degrees: Option<f32>,
    adapter: SelectedAdapter,
}

impl SelectedSimulatorAdapter {
    pub fn new(status: SharedCaptureStatus, steering_lock_degrees: Option<f32>) -> Self {
        let selected_id = selected_id(&status);
        let adapter = make_adapter(&selected_id, steering_lock_degrees);
        Self {
            identity: AdapterIdentity {
                key: "selected-simulator".into(),
                display_name: "Selected simulator".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            status,
            selected_id,
            steering_lock_degrees,
            adapter,
        }
    }

    fn refresh_selection(&mut self) {
        let selected_id = selected_id(&self.status);
        if selected_id != self.selected_id {
            self.adapter = make_adapter(&selected_id, self.steering_lock_degrees);
            self.selected_id = selected_id;
        }
    }
}

impl SimulatorAdapter for SelectedSimulatorAdapter {
    fn identity(&self) -> &AdapterIdentity {
        &self.identity
    }

    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        self.refresh_selection();
        self.adapter.poll()
    }
}

fn selected_id(status: &SharedCaptureStatus) -> String {
    status
        .lock()
        .map_or_else(|_| "auto".into(), |value| value.simulator_id.clone())
}

fn make_adapter(id: &str, steering_lock_degrees: Option<f32>) -> SelectedAdapter {
    match id {
        "auto" => SelectedAdapter::Auto(Box::new(AutoAdapter::new(steering_lock_degrees))),
        "le-mans-ultimate" => SelectedAdapter::LeMansUltimate(LmuAdapter::new()),
        _ => SelectedAdapter::AssettoCorsa(AcAdapter::with_steering_lock_degrees(
            steering_lock_degrees,
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use trace_adapter::DisconnectReason;

    use super::*;

    struct FakeAdapter {
        identity: AdapterIdentity,
        polls: VecDeque<Result<Vec<AdapterEvent>, AdapterError>>,
    }

    impl FakeAdapter {
        fn new(events: Vec<Vec<AdapterEvent>>) -> Self {
            Self {
                identity: AdapterIdentity {
                    key: "fake".into(),
                    display_name: "Fake".into(),
                    version: "1".into(),
                },
                polls: events.into_iter().map(Ok).collect(),
            }
        }
    }

    impl SimulatorAdapter for FakeAdapter {
        fn identity(&self) -> &AdapterIdentity {
            &self.identity
        }

        fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
            self.polls.pop_front().unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    #[test]
    fn auto_selection_builds_the_detector() {
        assert!(matches!(
            make_adapter("auto", None),
            SelectedAdapter::Auto(_)
        ));
    }

    #[test]
    fn auto_selection_releases_a_disconnected_simulator() {
        let mut adapter = AutoAdapter::new(None);
        adapter.active = Some(ActiveSimulator::LeMansUltimate);

        adapter.clear_active_if_disconnected(&Ok(vec![AdapterEvent::Disconnected(
            DisconnectReason::SessionEnded,
        )]));

        assert!(adapter.active.is_none());
    }

    #[test]
    fn auto_selection_keeps_a_simulator_while_it_is_active() {
        let mut adapter = AutoAdapter::new(None);
        adapter.active = Some(ActiveSimulator::AssettoCorsa);

        adapter.clear_active_if_disconnected(&Ok(Vec::new()));

        assert!(matches!(
            adapter.active,
            Some(ActiveSimulator::AssettoCorsa)
        ));
    }

    #[test]
    fn auto_selection_detects_the_second_available_simulator() {
        let assetto_corsa = FakeAdapter::new(vec![Vec::new()]);
        let le_mans_ultimate = FakeAdapter::new(vec![vec![AdapterEvent::Paused]]);
        let mut adapter = AutoAdapter::with_adapters(assetto_corsa, le_mans_ultimate);

        assert!(
            matches!(adapter.poll(), Ok(events) if matches!(events.as_slice(), [AdapterEvent::Paused]))
        );
        assert!(matches!(
            adapter.active,
            Some(ActiveSimulator::LeMansUltimate)
        ));
    }

    #[test]
    fn auto_selection_stays_with_the_detected_simulator() {
        let assetto_corsa = FakeAdapter::new(vec![
            vec![AdapterEvent::Paused],
            vec![AdapterEvent::Resumed],
        ]);
        let le_mans_ultimate = FakeAdapter::new(vec![vec![AdapterEvent::Paused]]);
        let mut adapter = AutoAdapter::with_adapters(assetto_corsa, le_mans_ultimate);

        assert!(
            matches!(adapter.poll(), Ok(events) if matches!(events.as_slice(), [AdapterEvent::Paused]))
        );
        assert!(
            matches!(adapter.poll(), Ok(events) if matches!(events.as_slice(), [AdapterEvent::Resumed]))
        );
        assert_eq!(adapter.le_mans_ultimate.polls.len(), 1);
    }
}
