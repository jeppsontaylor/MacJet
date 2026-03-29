//! Filesystem watch — `notify-debouncer-full` with `PollWatcher` fallback.

use crate::disk::model::DiskUiEvent;
use crossbeam_channel::Sender;
use notify_debouncer_full::{
    new_debouncer, new_debouncer_opt,
    notify::{Config, PollWatcher, RecursiveMode},
    DebounceEventResult, RecommendedCache,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub fn spawn_watcher(
    root: PathBuf,
    cancel: Arc<AtomicBool>,
    ui_tx: Sender<DiskUiEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let handler = |ui: Sender<DiskUiEvent>| {
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    if !events.is_empty() {
                        let _ = ui.send(DiskUiEvent::WatchSuggested);
                    }
                }
            }
        };

        if let Ok(mut debouncer) =
            new_debouncer(Duration::from_secs(2), None, handler(ui_tx.clone()))
        {
            if debouncer
                .watch(root.as_path(), RecursiveMode::Recursive)
                .is_ok()
            {
                while !cancel.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(400));
                }
                return;
            }
        }

        let Ok(mut debouncer) = new_debouncer_opt::<_, PollWatcher, _>(
            Duration::from_secs(2),
            None,
            handler(ui_tx),
            RecommendedCache::new(),
            Config::default(),
        ) else {
            return;
        };

        if debouncer
            .watch(root.as_path(), RecursiveMode::Recursive)
            .is_err()
        {
            return;
        }

        while !cancel.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(400));
        }
    })
}
