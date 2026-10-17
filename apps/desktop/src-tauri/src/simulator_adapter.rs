use trace_ac::AcAdapter;
use trace_adapter::{AdapterError, AdapterEvent, AdapterIdentity, SimulatorAdapter};
use trace_lmu::LmuAdapter;

use crate::capture::SharedCaptureStatus;

enum SelectedAdapter {
    AssettoCorsa(AcAdapter),
    LeMansUltimate(LmuAdapter),
}

impl SelectedAdapter {
    fn poll(&mut self) -> Result<Vec<AdapterEvent>, AdapterError> {
        match self {
            Self::AssettoCorsa(adapter) => adapter.poll(),
            Self::LeMansUltimate(adapter) => adapter.poll(),
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
    status.lock().map_or_else(
        |_| "assetto-corsa".into(),
        |value| value.simulator_id.clone(),
    )
}

fn make_adapter(id: &str, steering_lock_degrees: Option<f32>) -> SelectedAdapter {
    match id {
        "le-mans-ultimate" => SelectedAdapter::LeMansUltimate(LmuAdapter::new()),
        _ => SelectedAdapter::AssettoCorsa(AcAdapter::with_steering_lock_degrees(
            steering_lock_degrees,
        )),
    }
}
