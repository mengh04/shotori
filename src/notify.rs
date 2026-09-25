//! # Desktop notifications: fire-and-forget via a detached child process
//!
//! Why a child process: shotori calls `cx.quit()` right after saving / OCR.
//! A plain background thread would be killed mid-send when the process
//! exits; a detached child (`shotori --notify <summary> <body>`, same
//! pattern as the clipboard daemon) outlives the parent and always
//! delivers. Failures are silent by design — a missing notification daemon
//! must never break a screenshot tool.

use std::process::{Command, Stdio};

/// The argv marker for the notify child (main.rs dispatches on this)
pub const NOTIFY_ARG: &str = "--notify";

/// Queue a notification and return immediately. Never fails loudly:
/// the child prints to stderr only if something is actually broken.
pub fn send(summary: &str, body: &str) {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    let _ = Command::new(exe)
        .arg(NOTIFY_ARG)
        .arg(summary)
        .arg(body)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // stderr inherited: real failures stay visible in the terminal
        .spawn();
    // Detached: dropping the handle leaves the child running; the parent
    // usually exits a moment later and init reaps it.
}

/// Child entry point: `shotori --notify <summary> <body>` → show → exit.
pub fn notify_main() -> i32 {
    let mut args = std::env::args().skip(2);
    let (Some(summary), Some(body)) = (args.next(), args.next()) else {
        eprintln!("[shotori] notify child: usage: --notify <summary> <body>");
        return 1;
    };
    match notify_rust::Notification::new()
        .appname("Shotori")
        .summary(&summary)
        .body(&body)
        .timeout(notify_rust::Timeout::Milliseconds(3500))
        .show()
    {
        Ok(_) => 0,
        Err(e) => {
            // No daemon on the bus, session bus missing, … — not fatal for
            // the caller (the action already succeeded), just report it.
            eprintln!("[shotori] notification not delivered: {e}");
            1
        }
    }
}