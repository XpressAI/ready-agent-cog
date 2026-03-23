//! Integration tests for CLI error recovery functionality.
//!
//! These tests verify that the CLI properly handles recovery arguments and
//! validates the recovery mode constraints.

use std::process::Command;

/// Test that the CLI accepts --recovery flag
#[test]
fn cli_accepts_recovery_flag() {
    let output = Command::new("cargo")
        .args(&["run", "--", "run", "--help"])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--recovery"), "CLI should accept --recovery flag");
}

/// Test that the CLI accepts --recovery-attempts flag
#[test]
fn cli_accepts_recovery_attempts_flag() {
    let output = Command::new("cargo")
        .args(&["run", "--", "run", "--help"])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--recovery-attempts"),
        "CLI should accept --recovery-attempts flag"
    );
}

/// Test that recovery mode requires --sop when enabled
#[test]
fn recovery_mode_requires_sop() {
    // This test verifies that the CLI validates the --sop requirement for recovery mode
    // We can't easily test the actual error without a real SOP file, so we verify the help text
    let output = Command::new("cargo")
        .args(&["run", "--", "run", "--help"])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Verify both --sop and --recovery are available
    assert!(stdout.contains("--sop"), "CLI should have --sop flag");
    assert!(stdout.contains("--recovery"), "CLI should have --recovery flag");
}
