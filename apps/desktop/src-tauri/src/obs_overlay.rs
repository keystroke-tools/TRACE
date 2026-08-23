use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

use serde_json::json;

use crate::{SharedCaptureStatus, percentage, simulator_name};

pub const OBS_OVERLAY_ADDRESS: &str = "127.0.0.1:18081";

const OVERLAY_HTML: &str = include_str!("pedal-overlay.html");

pub fn spawn(status: SharedCaptureStatus) {
    thread::Builder::new()
        .name("trace-obs-overlay".into())
        .spawn(move || serve(status))
        .expect("failed to start TRACE OBS overlay worker");
}

fn serve(status: SharedCaptureStatus) {
    let listener = match TcpListener::bind(OBS_OVERLAY_ADDRESS) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("TRACE OBS overlay endpoint unavailable: {error}");
            return;
        }
    };
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => respond(stream, &status),
            Err(error) => eprintln!("TRACE OBS overlay request failed: {error}"),
        }
    }
}

fn respond(mut stream: TcpStream, status: &SharedCaptureStatus) {
    let mut request = [0_u8; 4096];
    let Ok(read) = stream.read(&mut request) else {
        return;
    };
    let request = String::from_utf8_lossy(&request[..read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");

    match path {
        "/overlays/pedals" | "/overlays/pedals/" => write_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            OVERLAY_HTML.as_bytes(),
        ),
        "/api/overlays/pedals" => {
            let snapshot = status
                .lock()
                .map_or_else(|_| crate::CaptureStatus::default(), |value| value.clone());
            let body = serde_json::to_vec(&json!({
                "connection": snapshot.connection,
                "simulatorName": simulator_name(&snapshot.simulator_id),
                "session": snapshot.session,
                "sequence": snapshot.live_inputs.sequence,
                "throttlePercent": percentage(snapshot.live_inputs.throttle),
                "brakePercent": percentage(snapshot.live_inputs.brake),
                "clutchPercent": percentage(snapshot.live_inputs.clutch),
                "steeringDegrees": snapshot.live_inputs.steering_angle_rad
                    .filter(|value| value.is_finite())
                    .map(f32::to_degrees),
            }))
            .unwrap_or_else(|_| b"{}".to_vec());
            write_response(&mut stream, "200 OK", "application/json", &body);
        }
        _ => write_response(&mut stream, "404 Not Found", "text/plain", b"Not found"),
    }
}

fn write_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(headers.as_bytes());
    let _ = stream.write_all(body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_source_is_self_contained() {
        assert!(OVERLAY_HTML.contains("class=\"graph\""));
        assert!(OVERLAY_HTML.contains("/api/overlays/pedals"));
    }
}
