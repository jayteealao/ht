/// Golden tests for asciicast v3 format compliance
///
/// These tests validate our implementation against the official asciicast v3
/// specification and known-good examples.
use super::*;
use serde_json::Value;
use std::io::{BufRead, BufReader};

#[test]
fn test_golden_minimal_cast_structure() {
    // Load the golden fixture
    let golden_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/golden_minimal.cast");

    let file = File::open(&golden_path)
        .expect("Golden file should exist - run from repo root");

    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    // Line 1: Header
    let header_line = lines.next().unwrap().unwrap();
    let header: Value = serde_json::from_str(&header_line).unwrap();

    assert_eq!(header["version"], 3, "version must be 3");
    assert!(header["term"].is_object(), "term must be object");
    assert_eq!(header["term"]["cols"], 80);
    assert_eq!(header["term"]["rows"], 24);

    // Lines 2+: Events
    let mut event_count = 0;
    let mut last_event_code = String::new();

    for line_result in lines {
        let line = line_result.unwrap();
        let event: Value = serde_json::from_str(&line).unwrap();

        assert!(event.is_array(), "Event must be array");
        let arr = event.as_array().unwrap();
        assert_eq!(arr.len(), 3, "Event must have 3 elements");

        let interval = arr[0].as_f64().unwrap();
        let code = arr[1].as_str().unwrap();
        let data = &arr[2];

        assert!(interval >= 0.0, "Interval must be non-negative");

        match code {
            "o" => {
                assert!(data.is_string(), "Output data must be string");
            }
            "m" => {
                assert!(data.is_string(), "Marker data must be string");
            }
            "r" => {
                assert!(data.is_string(), "Resize data must be string");
                let resize_str = data.as_str().unwrap();
                assert!(
                    resize_str.contains('x'),
                    "Resize must be COLSxROWS format"
                );
            }
            "x" => {
                // CRITICAL: Exit status must be a NUMBER, not a string!
                assert!(
                    data.is_number() || data.is_string(),
                    "Exit data type unclear from spec"
                );
                // Our current implementation uses string - need to verify
                last_event_code = code.to_string();
            }
            _ => panic!("Unknown event code: {}", code),
        }

        event_count += 1;
    }

    assert!(event_count > 0, "Should have at least one event");
    assert_eq!(last_event_code, "x", "Last event should be exit");
}

#[test]
fn test_exit_event_data_type() {
    // This test VERIFIES the critical question: is exit status a number or string?
    let golden_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/golden_minimal.cast");

    let file = File::open(&golden_path).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().collect();

    // Last line should be exit event
    let exit_line = lines.last().unwrap().as_ref().unwrap();
    let event: Value = serde_json::from_str(exit_line).unwrap();
    let arr = event.as_array().unwrap();

    assert_eq!(arr[1], "x", "Should be exit event");

    // THE CRITICAL ASSERTION:
    let exit_data = &arr[2];
    let is_number = exit_data.is_number();
    let is_string = exit_data.is_string();

    eprintln!(
        "Exit event data: {:?}, is_number: {}, is_string: {}",
        exit_data, is_number, is_string
    );

    // Based on spec: "data is a numerical exit status"
    // But JSON can represent numbers as strings in some cases
    // The golden file has: [0.887, "x", 0] - an INTEGER
    assert!(
        is_number,
        "Exit status should be a JSON number per spec 'numerical exit status'"
    );
}

#[test]
fn test_our_writer_no_spurious_init_output() {
    // Verify we DON'T emit init seq as first output event
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("test_no_init_{}.cast", uuid::Uuid::new_v4()));

    let config = RecorderConfig {
        output_path: test_file.clone(),
        append: false,
        idle_time_limit: None,
        title: None,
        command: None,
        capture_env: vec![],
        theme: None,
        term_type: None,
        capture_input: false,
    };

    let mut recorder = AsciicastV3Recorder::new(config).unwrap();

    // Simulate Init event
    recorder
        .handle_event(Event::Init(
            0.0,
            80,
            24,
            1234,
            "initial state".to_string(),
            "text view".to_string(),
        ))
        .unwrap();

    // Simulate first real output
    recorder
        .handle_event(Event::Output(0.1, "hello\n".to_string()))
        .unwrap();

    recorder.flush().unwrap();

    // Read back and verify
    let file = File::open(&test_file).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().map(|l| l.unwrap()).collect();

    assert!(lines.len() >= 2, "Should have header + at least 1 event");

    // First line is header
    let header: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(header["version"], 3);

    // CRITICAL: Second line should be the actual output, NOT init seq
    let first_event: Value = serde_json::from_str(&lines[1]).unwrap();
    let arr = first_event.as_array().unwrap();
    assert_eq!(arr[1], "o", "First event should be output");
    let data = arr[2].as_str().unwrap();

    // FAILS if we emit init seq as first event:
    assert_ne!(
        data, "initial state",
        "Init seq should NOT be emitted as first output event"
    );
    assert_eq!(
        data, "hello\n",
        "First output event should be actual terminal output"
    );

    std::fs::remove_file(test_file).ok();
}

#[test]
fn test_interval_monotonicity() {
    // Verify intervals are always >= 0
    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join(format!("test_monotonic_{}.cast", uuid::Uuid::new_v4()));

    let config = RecorderConfig {
        output_path: test_file.clone(),
        append: false,
        idle_time_limit: None,
        title: None,
        command: None,
        capture_env: vec![],
        theme: None,
        term_type: None,
        capture_input: false,
    };

    let mut recorder = AsciicastV3Recorder::new(config).unwrap();

    // Simulate rapid events
    recorder
        .handle_event(Event::Init(0.0, 80, 24, 1234, "".to_string(), "".to_string()))
        .unwrap();

    for i in 0..100 {
        recorder
            .handle_event(Event::Output(i as f64 * 0.001, format!("line {}\n", i)))
            .unwrap();
    }

    recorder.flush().unwrap();

    // Read back and check
    let file = File::open(&test_file).unwrap();
    let reader = BufReader::new(file);
    let lines: Vec<_> = reader.lines().skip(1).collect(); // Skip header

    for line_result in lines {
        let line = line_result.unwrap();
        let event: Value = serde_json::from_str(&line).unwrap();
        let arr = event.as_array().unwrap();
        let interval = arr[0].as_f64().unwrap();

        assert!(
            interval >= 0.0,
            "Interval must be non-negative, got {}",
            interval
        );
        assert!(
            interval.is_finite(),
            "Interval must be finite, got {}",
            interval
        );
    }

    std::fs::remove_file(test_file).ok();
}
