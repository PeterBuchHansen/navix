// navix - off-thread host graphics writer.
// Copyright (C) 2026 Peter Buch Hansen
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

/// Off-loads blocking host-terminal graphics writes (kitty / iTerm2 image
/// transfers) to a dedicated thread so the main event loop never blocks on
/// terminal back-pressure.
///
/// A multi-megabyte image can take the host hundreds of milliseconds to accept
/// and decode (e.g. a 13 MB uncompressed RGBA frame at ~50 MB/s ≈ 260 ms). Done
/// inline that froze input, scrolling, and `Ctrl+C` for the whole write — a
/// nested tool such as a pager, notebook, or image viewer all hit the same
/// path, so this is a protocol- and tool-independent fix.
///
/// A single graphics transfer is one indivisible escape sequence: if a ratatui
/// frame's text/escapes are written into the middle of it the image corrupts.
/// Each blob is therefore written under a held `StdoutLock`, and the main thread
/// consults [`GraphicsWriter::is_busy`] and defers its own repaint until the
/// queue drains — which also preserves placement, since the image finishes
/// loading before the placeholder cells that position it are drawn.
pub(crate) struct GraphicsWriter {
    tx: Option<Sender<Vec<u8>>>,
    // Count of blobs queued but not yet fully written. Incremented by the main
    // thread on `send`, decremented by the writer thread once a blob drains.
    pending: Arc<AtomicUsize>,
    handle: Option<JoinHandle<()>>,
}

impl GraphicsWriter {
    pub(crate) fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let pending = Arc::new(AtomicUsize::new(0));
        let thread_pending = Arc::clone(&pending);
        let handle = thread::Builder::new()
            .name("navix-graphics-writer".to_string())
            .spawn(move || {
                let stdout = io::stdout();
                while let Ok(blob) = rx.recv() {
                    {
                        // Hold the lock across the whole blob so no other
                        // stdout writer can interleave bytes into the transfer.
                        let mut lock = stdout.lock();
                        let _ = lock.write_all(&blob);
                        let _ = lock.flush();
                    }
                    thread_pending.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .expect("spawn graphics writer thread");
        Self {
            tx: Some(tx),
            pending,
            handle: Some(handle),
        }
    }

    /// Queue a graphics blob for the host terminal. Empty blobs are dropped so
    /// idle frames never mark the writer busy.
    pub(crate) fn send(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Some(tx) = &self.tx {
            self.pending.fetch_add(1, Ordering::SeqCst);
            if tx.send(bytes).is_err() {
                self.pending.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }

    /// True while any queued or in-flight blob has not finished writing. The
    /// main thread must not paint the host terminal while this holds.
    pub(crate) fn is_busy(&self) -> bool {
        self.pending.load(Ordering::SeqCst) > 0
    }

    /// Close the channel and join the writer, flushing every queued blob first.
    /// Must run before the terminal is restored so no write races teardown.
    pub(crate) fn shutdown(&mut self) {
        self.tx = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for GraphicsWriter {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn empty_blob_never_marks_busy() {
        let writer = GraphicsWriter::spawn();
        writer.send(Vec::new());
        assert!(!writer.is_busy());
    }

    #[test]
    fn drains_and_clears_busy_after_shutdown() {
        let mut writer = GraphicsWriter::spawn();
        // Writing to stdout in tests is harmless; the bytes are tiny.
        writer.send(b"\x1b_Ga=q;\x1b\\".to_vec());
        writer.shutdown();
        // After join, every queued blob has drained.
        assert!(!writer.is_busy());
    }

    #[test]
    fn busy_returns_to_idle_without_shutdown() {
        let writer = GraphicsWriter::spawn();
        writer.send(b"hello".to_vec());
        let deadline = Instant::now() + Duration::from_secs(5);
        while writer.is_busy() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            !writer.is_busy(),
            "writer should drain a small blob promptly"
        );
    }
}
