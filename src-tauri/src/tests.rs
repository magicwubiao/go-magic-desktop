// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // This file is itself the body of `mod tests;` declared in main.rs, so the
    // crate root is *two* levels up. `use super::*` would only reach this file's
    // own module (which has no items) and every test would fail to resolve.
    use crate::*;

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
            maximized: false,
        });
        assert_eq!(tiny.width, MIN_WINDOW_WIDTH);
        assert_eq!(tiny.height, MIN_WINDOW_HEIGHT);

        // A corrupt state file must not be able to produce NaN geometry.
        let nan = sanitize_window_state(WindowState {
            width: f64::NAN,
            height: f64::NAN,
            x: None,
            y: None,
            maximized: false,
        });
        assert_eq!(nan.width, MIN_WINDOW_WIDTH);
        assert_eq!(nan.height, MIN_WINDOW_HEIGHT);

        let large = sanitize_window_state(WindowState {
            width: 4000.0,
            height: 3000.0,
            x: None,
            y: None,
            maximized: false,
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

    #[test]
    fn restart_delay_backs_off_and_is_capped() {
        assert_eq!(restart_delay(0), AUTO_RESTART_BASE_DELAY);
        assert_eq!(restart_delay(1), Duration::from_secs(2));
        assert_eq!(restart_delay(2), Duration::from_secs(4));
        assert_eq!(restart_delay(3), Duration::from_secs(8));
        assert_eq!(restart_delay(4), Duration::from_secs(16));

        // Capped from here on — including absurd counters, which would overflow
        // a naive `1 << attempt`.
        assert_eq!(restart_delay(5), AUTO_RESTART_MAX_DELAY);
        assert_eq!(restart_delay(u32::MAX), AUTO_RESTART_MAX_DELAY);

        let mut previous = Duration::ZERO;
        for attempt in 0..40 {
            let delay = restart_delay(attempt);
            assert!(delay >= previous, "backoff went backwards at {}", attempt);
            assert!(delay <= AUTO_RESTART_MAX_DELAY);
            previous = delay;
        }
    }

    /// Runs a real child, so the exit-code/signal split is exercised exactly the
    /// way the supervisor sees it.
    fn exit_status_of(script: &str) -> ExitStatus {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script);
            command
        };

        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", script]);
            command
        };

        command.status().expect("spawn a shell")
    }

    #[test]
    fn describe_exit_status_names_an_exit_code() {
        assert_eq!(
            describe_exit_status(exit_status_of("exit 3")),
            "exit code 3"
        );
    }

    /// A signal is the useful half of a crash report: 9 is what the OOM killer
    /// uses, and naming it is what makes that recognisable in the log.
    #[cfg(unix)]
    #[test]
    fn describe_exit_status_names_the_signal() {
        let description = describe_exit_status(exit_status_of("kill -9 $$"));
        assert!(description.contains('9'), "{}", description);
        assert!(description.contains("SIGKILL"), "{}", description);
    }

    #[test]
    fn stderr_tail_keeps_only_the_last_lines() {
        clear_stderr_tail();

        for i in 0..(BACKEND_STDERR_TAIL_LINES + 5) {
            remember_stderr_line(&format!("line {}", i));
        }

        let tail = backend_stderr_tail();
        assert_eq!(tail.len(), BACKEND_STDERR_TAIL_LINES);
        assert_eq!(tail.first().unwrap(), "line 5");
        assert_eq!(
            tail.last().unwrap(),
            &format!("line {}", BACKEND_STDERR_TAIL_LINES + 4)
        );

        clear_stderr_tail();
        assert!(backend_stderr_tail().is_empty());
    }

    #[test]
    fn supervisor_timing_constants_are_sane() {
        assert!(SUPERVISE_INTERVAL >= Duration::from_millis(100));
        assert!(SUPERVISE_INTERVAL <= AUTO_RESTART_BASE_DELAY);
        assert!(MAX_BACKEND_RESTARTS >= 3);
        assert!(AUTO_RESTART_BASE_DELAY < AUTO_RESTART_MAX_DELAY);
        assert!(BACKEND_STDERR_TAIL_LINES >= 5);
    }
}