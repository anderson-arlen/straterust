use super::*;
#[test]
fn fuzzy_console_completion_executes_debug_controls_without_gameplay_input() {
    let mut debug = Diagnostics::default();
    assert!(!debug.edit(KeyCode::KeyA, Some("a"), false));
    assert!(debug.edit(KeyCode::Backquote, Some("`"), false));
    debug.edit(KeyCode::KeyD, Some("dbg of"), false);
    assert_eq!(debug.suggestions()[0], "debug off");
    debug.edit(KeyCode::Tab, None, false);
    assert_eq!(debug.input, "debug off");
    debug.edit(KeyCode::Enter, None, false);
    assert!(!debug.fps && !debug.network);
    debug.edit(KeyCode::KeyD, Some("debug on"), false);
    debug.edit(KeyCode::Enter, None, false);
    assert!(debug.fps && debug.network);
    debug.edit(KeyCode::ArrowUp, None, false);
    assert_eq!(debug.input, "debug on");
    debug.edit(KeyCode::Escape, None, false);
    assert!(!debug.open);
}
#[test]
fn samples_count_actual_time_and_wire_bytes_and_console_slides_to_half_height() {
    let mut debug = Diagnostics {
        open: true,
        ..Diagnostics::default()
    };
    for i in 1..=10 {
        debug.sample(Duration::from_millis(20), [i * 100, i * 200], i);
    }
    assert_eq!(debug.height, 0.5);
    assert_eq!(debug.current.fps, 50.);
    assert_eq!(debug.current.up, 5000.);
    assert_eq!(debug.current.down, 10000.);
    debug.open = false;
    debug.sample(Duration::from_millis(500), [1000, 2000], 11);
    assert_eq!(debug.height, 0.);
    assert_eq!(debug.current.up, 0.);
    assert_eq!(debug.current.down, 0.);
    assert!(debug.current.fps < 3.);
    for i in 0..150 {
        debug.sample(Duration::from_millis(200), [1000, 2000], i);
    }
    assert_eq!(debug.samples.len(), 120);
}
#[test]
fn unicode_editing_and_fuzzy_completion_are_bounded() {
    let mut debug = Diagnostics {
        open: true,
        ..Diagnostics::default()
    };
    debug.edit(KeyCode::KeyA, Some("aéz"), false);
    debug.edit(KeyCode::ArrowLeft, None, false);
    debug.edit(KeyCode::Backspace, None, false);
    assert_eq!(debug.input, "az");
    assert_eq!(debug.caret, 1);
    debug.edit(KeyCode::KeyD, Some(&"d".repeat(300)), false);
    assert_eq!(debug.input.chars().count(), 256);
    assert!(debug.suggestions().is_empty());
}
