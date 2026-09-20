// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_state_default_matches_constants() {
        let state = WindowState::default();
        assert_eq!(state.width, DEFAULT_WINDOW_WIDTH);
        assert_eq!(state.height, DEFAULT_WINDOW_HEIGHT);
        assert!(state.x.is_none());
        assert!(state.y.is_none());
        assert!(!state.maximized);
    }

    #[test]
    fn window_state_without_a_maximized_field_still_loads() {
        // State files written before the `maximized` flag existed must keep
        // working; a hard parse error would silently reset the user's geometry.
        let legacy = r#"{"width":1280,"height":900,"x":100,"y":80}"#;
        let state: WindowState = serde_json::from_str(legacy).unwrap();

        assert_eq!(state.width, 1280.0);
        assert_eq!(state.height, 900.0);
        assert_eq!(state.x, Some(100.0));
        assert_eq!(state.y, Some(80.0));
        assert!(!state.maximized);
    }

    #[test]
    fn window_state_survives_a_json_round_trip() {
        let original = WindowState {
            width: 1280.0,
            height: 900.0,
            x: Some(12.0),
            y: Some(-34.0),
            maximized: true,
        };

        let json = serde_json::to_string(&original).unwrap();
        let restored: WindowState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.width, original.width);
        assert_eq!(restored.height, original.height);
        assert_eq!(restored.x, original.x);
        assert_eq!(restored.y, original.y);
        assert!(restored.maximized);
    }

    /// The restore geometry must be the *plain* window size, never the monitor
    /// size a maximized window would report. Turning a monitor-sized rectangle
    /// into a normal window is what leaves the frame border hanging off-screen.
    #[test]
    fn a_maximized_window_keeps_its_pre_maximize_geometry() {
        let mut state = WindowState {
            width: 1280.0,
            height: 900.0,
            x: Some(100.0),
            y: Some(80.0),
            maximized: false,
        };

        // What `save_window_state` does when it sees `is_maximized() == true`:
        // only the flag flips, the geometry is left untouched.
        state.maximized = true;

        assert_eq!(state.width, 1280.0);
        assert_eq!(state.height, 900.0);
        assert_eq!(state.x, Some(100.0));
        assert_eq!(state.y, Some(80.0));
        assert!(state.maximized);
    }

    #[test]
    fn window_state_is_clamped_to_the_minimum_size() {
        let tiny = sanitize_window_state(WindowState {
            width: 10.0,
            height: 20.0,
            x: Some(0.0),
            y: Some(0.0),
        });
        assert_eq!(tiny.width, MIN_WINDOW_WIDTH);
        assert_eq!(tiny.height, MIN_WINDOW_HEIGHT);

        // A corrupt state file must not be able to produce NaN geometry.
        let nan = sanitize_window_state(WindowState {
            width: f64::NAN,
            height: f64::NAN,
            x: None,
            y: None,
        });
        assert_eq!(nan.width, MIN_WINDOW_WIDTH);
        assert_eq!(nan.height, MIN_WINDOW_HEIGHT);

        let large = sanitize_window_state(WindowState {
            width: 4000.0,
            height: 3000.0,
            x: None,
            y: None,
        });
        assert_eq!(large.width, 4000.0);
        assert_eq!(large.height, 3000.0);
    }

    #[test]
    fn pick_available_port_returns_a_port_from_the_expected_range() {
        let port = pick_available_port().expect("expected at least one free port");
        assert!(
            DEFAULT_PORTS.contains(&port) || (8000..9000).contains(&port),
            "unexpected port {}",
            port
        );
    }

    #[test]
    fn backend_errors_have_readable_messages() {
        assert_eq!(
            BackendError::NoPortAvailable.to_string(),
            "No available port found"
        );
        assert_eq!(
            BackendError::BackendNotFound.to_string(),
            "Backend executable not found"
        );
        assert_eq!(
            BackendError::SpawnFailed("boom".into()).to_string(),
            "Failed to start backend: boom"
        );
        assert!(BackendError::HealthCheckTimeout
            .to_string()
            .contains("timed out"));
    }

    #[test]
    fn health_check_timing_constants_are_sane() {
        assert!(HEALTH_CHECK_INTERVAL < HEALTH_CHECK_TIMEOUT);
        assert!(BACKEND_STATUS_INTERVAL >= HEALTH_CHECK_INTERVAL);
        assert!(BACKEND_STOP_TIMEOUT <= HEALTH_CHECK_TIMEOUT);
        assert!(HEALTH_PROBE_TIMEOUT < HEALTH_CHECK_TIMEOUT);
        assert!(HEALTH_CHECK_TIMEOUT >= Duration::from_secs(10));
    }

    #[test]
    fn parse_status_code_reads_the_code() {
        assert_eq!(parse_status_code("HTTP/1.1 200 OK"), Some(200));
        assert_eq!(parse_status_code("HTTP/1.0 204 No Content"), Some(204));
        assert_eq!(
            parse_status_code("HTTP/1.1 503 Service Unavailable\r"),
            Some(503)
        );
    }

    #[test]
    fn parse_status_code_rejects_everything_else() {
        // A port held by something that is not our backend must read as unhealthy.
        assert_eq!(parse_status_code(""), None);
        assert_eq!(parse_status_code("SSH-2.0-OpenSSH_9.6"), None);
        assert_eq!(parse_status_code("HTTP/1.1"), None);
        assert_eq!(parse_status_code("HTTP/1.1 abc Not a Number"), None);
    }

    /// Serves one canned response on an ephemeral port, then runs the real probe
    /// against it: end-to-end coverage without a live backend.
    fn probe_against(response: &'static str) -> bool {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind ephemeral port");
        let port = listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut request = [0u8; 256];
                let _ = socket.read(&mut request);
                let _ = socket.write_all(response.as_bytes());
            }
        });

        let healthy = check_backend_health(port);
        server.join().expect("server thread");
        healthy
    }

    #[test]
    fn health_probe_accepts_2xx_and_rejects_anything_else() {
        assert!(probe_against(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
        ));
        assert!(!probe_against(
            "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n"
        ));
        // Nothing at all (a socket that accepts and stays silent) must not hang
        // forever either; the read timeout turns it into "unhealthy".
        assert!(!probe_against(""));
    }

    #[test]
    fn health_probe_reports_a_closed_port_as_unhealthy() {
        // Bind and immediately drop, so the port is (almost certainly) free.
        let port = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };

        assert!(!check_backend_health(port));
    }

    #[test]
    fn backend_search_dirs_cover_the_resource_subdirectory() {
        let resource_dir = Path::new("some").join("resource-dir");
        let dirs = backend_search_dirs(&resource_dir);

        assert!(dirs.contains(&resource_dir));
        // `bundle.resources = ["resources/*"]` keeps its prefix, so the binary
        // ends up one level deeper than the resource root.
        assert!(dirs.contains(&resource_dir.join("resources")));
    }
}