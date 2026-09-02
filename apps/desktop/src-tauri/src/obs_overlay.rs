use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::Duration,
};

use serde_json::json;
use tauri::AppHandle;

use crate::{SharedCaptureStatus, percentage, simulator_name};

pub const OBS_OVERLAY_ADDRESS: &str = "127.0.0.1:18081";

const OVERLAY_HTML: &str = include_str!("pedal-overlay.html");

const TRACER_HEADER: &str = "x-trace-tracer: 1";
const MAX_REQUEST_BYTES: usize = 64 * 1024;

pub fn spawn(app: AppHandle, status: SharedCaptureStatus) {
    thread::Builder::new()
        .name("trace-obs-overlay".into())
        .spawn(move || serve(&app, &status))
        .expect("failed to start TRACE OBS overlay worker");
}

fn serve(app: &AppHandle, status: &SharedCaptureStatus) {
    let listener = match TcpListener::bind(OBS_OVERLAY_ADDRESS) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("TRACE OBS overlay endpoint unavailable: {error}");
            return;
        }
    };
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => respond(stream, app, status),
            Err(error) => eprintln!("TRACE OBS overlay request failed: {error}"),
        }
    }
}

fn respond(mut stream: TcpStream, app: &AppHandle, status: &SharedCaptureStatus) {
    let Ok(request) = read_request(&mut stream) else {
        return;
    };
    let request_text = String::from_utf8_lossy(&request);
    let request_line = request_text.lines().next().unwrap_or_default();
    let method = request_line.split_whitespace().next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");
    let authorized_tracer = authorized_tracer_request(&request_text);
    let body = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map_or(&[][..], |position| &request[position + 4..]);

    match (method, path) {
        ("GET", "/overlays/pedals" | "/overlays/pedals/") => write_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            OVERLAY_HTML.as_bytes(),
        ),
        ("GET", "/api/overlays/pedals") => {
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
        ("POST", "/api/tracer/sessions" | "/api/tracer/reference") if !authorized_tracer => {
            write_json_error(
                &mut stream,
                "403 Forbidden",
                "Tracer request was not authorized",
            );
        }
        ("POST", "/api/tracer/sessions") => {
            let result = serde_json::from_slice::<crate::tracer::TracerSessionQuery>(body)
                .map_err(|error| format!("invalid session query: {error}"))
                .and_then(|query| crate::tracer::matching_sessions(app, &query));
            write_json_result(&mut stream, result);
        }
        ("POST", "/api/tracer/reference") => {
            let result = serde_json::from_slice::<crate::tracer::TracerReferenceRequest>(body)
                .map_err(|error| format!("invalid reference request: {error}"))
                .and_then(|request| crate::tracer::activate_reference(app, &request));
            write_json_result(&mut stream, result);
        }
        _ => write_response(&mut stream, "404 Not Found", "text/plain", b"Not found"),
    }
}

fn authorized_tracer_request(request: &str) -> bool {
    request
        .lines()
        .take_while(|line| !line.is_empty() && *line != "\r")
        .any(|line| line.trim().eq_ignore_ascii_case(TRACER_HEADER))
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::with_capacity(4_096);
    let mut chunk = [0_u8; 4_096];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.len() > MAX_REQUEST_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "HTTP request exceeds the local endpoint limit",
            ));
        }
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        });
        if content_length.is_none_or(|length| request.len() >= header_end + 4 + length) {
            break;
        }
    }
    Ok(request)
}

fn write_json_result<T: serde::Serialize>(stream: &mut TcpStream, result: Result<T, String>) {
    match result {
        Ok(value) => {
            let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
            write_response(stream, "200 OK", "application/json", &body);
        }
        Err(error) => write_json_error(stream, "400 Bad Request", &error),
    }
}

fn write_json_error(stream: &mut TcpStream, status: &str, error: &str) {
    let body = serde_json::to_vec(&json!({ "error": error })).unwrap_or_else(|_| b"{}".to_vec());
    write_response(stream, status, "application/json", &body);
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

    #[test]
    fn tracer_bridge_requires_its_non_simple_request_header() {
        assert!(authorized_tracer_request(
            "POST /api/tracer/sessions HTTP/1.1\r\nX-Trace-Tracer: 1\r\n\r\n{}"
        ));
        assert!(!authorized_tracer_request(
            "POST /api/tracer/sessions HTTP/1.1\r\nContent-Type: text/plain\r\n\r\n{}"
        ));
    }
}
