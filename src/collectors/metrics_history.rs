/// MacJet — Metrics History & Reclaim Scoring Engine
///
/// Per-process ring buffers for sparklines, exponential smoothing for stable
/// sorting, and a multi-factor scoring engine for the Reclaim (Kill List) view.
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::collections::VecDeque;

const SPARK_CHARS: [char; 8] = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const BUFFER_SIZE: usize = 60; // 60 samples ≈ 60 seconds at 1s interval
const EXPIRY_S: f64 = 90.0;
const SMOOTH_ALPHA: f64 = 0.3;

#[derive(Debug, Clone)]
pub struct ProcessSample {
    pub timestamp: f64,
    pub cpu_percent: f64,
    pub memory_mb: f64,
}

#[derive(Debug, Clone)]
pub struct ReclaimCandidate {
    pub group_key: SmolStr,
    pub app_name: SmolStr,
    pub icon: &'static str,
    pub score: u8,
    pub reclaim_cpu: f64,
    pub reclaim_mem_mb: f64,
    pub risk: SmolStr,
    pub reason: SmolStr,
    pub suggested_action: SmolStr,
    pub child_count: usize,
    pub is_hidden: bool,
    pub launch_age_s: f64,
}

pub struct MetricsHistory {
    buffers: FxHashMap<u32, VecDeque<ProcessSample>>,
    smoothed_cpu: FxHashMap<u32, f64>,
    smoothed_mem: FxHashMap<u32, f64>,
    last_seen: FxHashMap<u32, f64>,
    // The current time is provided via parameters instead of an injected clock trait
    // to avoid trait object overhead on hot paths.
    current_time: f64,
}

impl Default for MetricsHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsHistory {
    pub fn new() -> Self {
        Self {
            buffers: FxHashMap::default(),
            smoothed_cpu: FxHashMap::default(),
            smoothed_mem: FxHashMap::default(),
            last_seen: FxHashMap::default(),
            current_time: 0.0,
        }
    }

    pub fn set_time(&mut self, now_s: f64) {
        self.current_time = now_s;
    }

    pub fn record(&mut self, pid: u32, cpu_percent: f64, memory_mb: f64) {
        let now = self.current_time;

        let buf = self.buffers.entry(pid).or_insert_with(|| {
            self.smoothed_cpu.insert(pid, cpu_percent);
            self.smoothed_mem.insert(pid, memory_mb);
            VecDeque::with_capacity(BUFFER_SIZE)
        });

        if buf.len() == BUFFER_SIZE {
            buf.pop_front();
        }

        buf.push_back(ProcessSample {
            timestamp: now,
            cpu_percent,
            memory_mb,
        });

        self.last_seen.insert(pid, now);

        let alpha = SMOOTH_ALPHA;
        if let Some(smoothed) = self.smoothed_cpu.get_mut(&pid) {
            *smoothed = alpha * cpu_percent + (1.0 - alpha) * *smoothed;
        }
        if let Some(smoothed) = self.smoothed_mem.get_mut(&pid) {
            *smoothed = alpha * memory_mb + (1.0 - alpha) * *smoothed;
        }
    }

    pub fn smoothed_cpu(&self, pid: u32) -> f64 {
        self.smoothed_cpu.get(&pid).copied().unwrap_or(0.0)
    }

    pub fn history(&self, pid: u32) -> Vec<f64> {
        if let Some(buf) = self.buffers.get(&pid) {
            buf.iter().map(|s| s.cpu_percent).collect()
        } else {
            Vec::new()
        }
    }

    pub fn smoothed_mem(&self, pid: u32) -> f64 {
        self.smoothed_mem.get(&pid).copied().unwrap_or(0.0)
    }

    pub fn sustained_cpu(&self, pid: u32, window_s: f64) -> f64 {
        if let Some(buf) = self.buffers.get(&pid) {
            let cutoff = self.current_time - window_s;
            let mut sum = 0.0;
            let mut count = 0;

            for sample in buf {
                if sample.timestamp >= cutoff {
                    sum += sample.cpu_percent;
                    count += 1;
                }
            }

            if count > 0 {
                return sum / count as f64;
            }
        }
        0.0
    }

    pub fn memory_growth_rate(&self, pid: u32, window_s: f64) -> f64 {
        if let Some(buf) = self.buffers.get(&pid) {
            if buf.len() < 2 {
                return 0.0;
            }

            let cutoff = self.current_time - window_s;
            let mut relevant = Vec::new();

            for sample in buf {
                if sample.timestamp >= cutoff {
                    relevant.push(sample);
                }
            }

            if relevant.len() < 2 {
                return 0.0;
            }

            let oldest = relevant.first().unwrap();
            let newest = relevant.last().unwrap();
            let dt = newest.timestamp - oldest.timestamp;

            if dt < 5.0 {
                return 0.0;
            }

            let dm = newest.memory_mb - oldest.memory_mb;
            return (dm / dt) * 60.0; // MB per minute
        }
        0.0
    }

    pub fn sparkline(&self, pid: u32, width: usize, metric: &str) -> String {
        if let Some(buf) = self.buffers.get(&pid) {
            if buf.is_empty() {
                return " ".repeat(width);
            }

            let values: Vec<f64> = if metric == "cpu" {
                buf.iter().map(|s| s.cpu_percent).collect()
            } else {
                buf.iter().map(|s| s.memory_mb).collect()
            };

            let mut max_val = 1.0;
            for &v in &values {
                if v > max_val {
                    max_val = v;
                }
            }

            let processed = Self::resample_and_pad(&values, width);
            Self::chars_from_values(&processed, max_val)
        } else {
            " ".repeat(width)
        }
    }

    pub fn sparkline_for_group(&self, pids: &[u32], width: usize) -> String {
        if pids.is_empty() {
            return " ".repeat(width);
        }

        let mut max_len = 0;
        for &pid in pids {
            if let Some(buf) = self.buffers.get(&pid) {
                if buf.len() > max_len {
                    max_len = buf.len();
                }
            }
        }

        if max_len == 0 {
            return " ".repeat(width);
        }

        let mut combined = vec![0.0; max_len];
        for &pid in pids {
            if let Some(buf) = self.buffers.get(&pid) {
                let offset = max_len - buf.len();
                for (i, s) in buf.iter().enumerate() {
                    combined[offset + i] += s.cpu_percent;
                }
            }
        }

        let mut max_val = 1.0;
        for &v in &combined {
            if v > max_val {
                max_val = v;
            }
        }

        let processed = Self::resample_and_pad(&combined, width);
        Self::chars_from_values(&processed, max_val)
    }

    fn resample_and_pad(values: &[f64], width: usize) -> Vec<f64> {
        let len = values.len();
        if len > width {
            let step = len as f64 / width as f64;
            let mut resampled = Vec::with_capacity(width);
            for i in 0..width {
                let mut idx = (i as f64 * step) as usize;
                if idx >= len {
                    idx = len - 1;
                }
                resampled.push(values[idx]);
            }
            resampled
        } else if len < width {
            let mut padded = vec![0.0; width - len];
            padded.extend_from_slice(values);
            padded
        } else {
            values.to_vec()
        }
    }

    fn chars_from_values(values: &[f64], max_val: f64) -> String {
        let mut chars = String::with_capacity(values.len());
        let levels = (SPARK_CHARS.len() - 1) as f64;

        for &v in values {
            let normalized = (v / max_val).min(1.0).max(0.0);
            let idx = (normalized * levels).floor() as usize;
            chars.push(SPARK_CHARS[idx.min(SPARK_CHARS.len() - 1)]);
        }
        chars
    }

    pub fn expire_stale(&mut self) {
        let now = self.current_time;
        // Collect stale PIDs
        let mut stale_pids = Vec::new();
        for (&pid, &ts) in &self.last_seen {
            if now - ts > EXPIRY_S {
                stale_pids.push(pid);
            }
        }

        // Remove them
        for pid in stale_pids {
            self.buffers.remove(&pid);
            self.smoothed_cpu.remove(&pid);
            self.smoothed_mem.remove(&pid);
            self.last_seen.remove(&pid);
        }
    }

    // ─── Reclaim Scoring ─────────────────────────────

    pub fn get_reclaim_candidates(
        &self,
        groups: &[crate::collectors::process_collector::ProcessGroup],
    ) -> Vec<ReclaimCandidate> {
        let mut candidates = Vec::new();
        for group in groups {
            let pids: Vec<u32> = group.processes.iter().map(|p| p.pid).collect();
            let has_high_wakeups = false; // Add actual wakeup tracking if needed

            // Aggregate metrics from processes
            let child_count = group.processes.len();
            let is_hidden = group.processes.iter().all(|p| p.is_hidden);
            let is_system = group.processes.iter().any(|p| p.is_system);
            let avg_launch_age = if child_count > 0 {
                group.processes.iter().map(|p| p.launch_age_s).sum::<f64>() / child_count as f64
            } else {
                0.0
            };

            let candidate = self.compute_reclaim_score(
                group.name.as_str(),
                group.name.as_str(),
                group.icon,
                &pids,
                group.total_cpu,
                group.total_memory_mb,
                child_count,
                is_hidden,
                is_system,
                has_high_wakeups,
                group.energy_impact.as_str(),
                avg_launch_age,
            );
            candidates.push(candidate);
        }

        candidates.sort_by(|a, b| b.score.cmp(&a.score));
        candidates
    }

    pub fn compute_reclaim_score(
        &self,
        group_key: &str,
        app_name: &str,
        icon: &'static str,
        pids: &[u32],
        total_cpu: f64,
        total_memory_mb: f64,
        child_count: usize,
        is_hidden: bool,
        is_system: bool,
        has_high_wakeups: bool,
        energy_impact: &str,
        launch_age_s: f64,
    ) -> ReclaimCandidate {
        let mut score: u32 = 0;

        // Factor 1: Sustained CPU (up to 30 points)
        let mut avg_sustained = 0.0;
        for &pid in pids {
            avg_sustained += self.sustained_cpu(pid, 30.0);
        }
        score += std::cmp::min(30, (avg_sustained * 0.3) as u32);

        // Factor 2: Memory footprint (up to 25 points)
        score += std::cmp::min(25, (total_memory_mb / 100.0) as u32);

        // Factor 3: Memory growth rate (up to 15 points)
        let mut total_growth = 0.0;
        for &pid in pids {
            let growth = self.memory_growth_rate(pid, 300.0);
            if growth > 0.0 {
                total_growth += growth;
            }
        }
        score += std::cmp::min(15, (total_growth * 3.0) as u32);

        // Factor 4: Hidden/background (15 points)
        if is_hidden {
            score += 15;
        }

        // Factor 5: Process storm (10 points)
        if child_count > 10 {
            score += 10;
        }

        // Factor 6: High wakeups / energy (5 points)
        if has_high_wakeups || energy_impact == "HIGH" {
            score += 5;
        }

        score = std::cmp::min(100, score);

        // Determine risk level
        let risk = if is_system {
            "danger"
        } else if !is_hidden && total_cpu > 5.0 {
            "review"
        } else {
            "safe"
        };

        let reason = Self::generate_reason(
            pids,
            total_cpu,
            total_memory_mb,
            child_count,
            is_hidden,
            has_high_wakeups,
            total_growth,
            launch_age_s,
        );

        let suggested_action = if risk == "danger" {
            "⚠ Review First"
        } else if is_hidden && total_cpu < 1.0 {
            "Quit App"
        } else if child_count > 10 {
            "Quit App"
        } else {
            "Terminate"
        };

        ReclaimCandidate {
            group_key: SmolStr::new(group_key),
            app_name: SmolStr::new(app_name),
            icon,
            score: score as u8,
            reclaim_cpu: total_cpu,
            reclaim_mem_mb: total_memory_mb,
            risk: SmolStr::new(risk),
            reason: SmolStr::new(reason),
            suggested_action: SmolStr::new(suggested_action),
            child_count,
            is_hidden,
            launch_age_s,
        }
    }

    fn generate_reason(
        _pids: &[u32],
        total_cpu: f64,
        total_memory_mb: f64,
        child_count: usize,
        is_hidden: bool,
        has_high_wakeups: bool,
        growth_rate: f64,
        launch_age_s: f64,
    ) -> String {
        let mut parts = Vec::new();

        if is_hidden && launch_age_s > 300.0 {
            let age_str = Self::format_duration(launch_age_s);
            parts.push(format!("Hidden {}", age_str));
        }

        if total_memory_mb > 500.0 {
            let mem_str = if total_memory_mb >= 1024.0 {
                format!("{:.1}G", total_memory_mb / 1024.0)
            } else {
                format!("{:.0}M", total_memory_mb)
            };
            parts.push(format!("{} resident", mem_str));
        }

        if growth_rate > 10.0 {
            parts.push(format!("+{:.0}MB/min growth", growth_rate));
        }

        if child_count > 10 {
            parts.push(format!("{} child processes", child_count));
        }

        if has_high_wakeups {
            parts.push("high wakeups".to_string());
        }

        if total_cpu > 50.0 {
            parts.push(format!("sustained {:.0}% CPU", total_cpu));
        } else if total_cpu < 1.0 && total_memory_mb > 200.0 {
            parts.push(format!("{:.1}% CPU", total_cpu));
        }

        if parts.is_empty() {
            parts.push(format!("{:.1}% CPU, {:.0}MB", total_cpu, total_memory_mb));
        }

        parts.join(", ")
    }

    fn format_duration(seconds: f64) -> String {
        if seconds < 60.0 {
            format!("{}s", seconds as u64)
        } else if seconds < 3600.0 {
            format!("{}m", (seconds / 60.0) as u64)
        } else {
            let h = (seconds / 3600.0) as u64;
            let m = ((seconds % 3600.0) / 60.0) as u64;
            if m > 0 {
                format!("{}h{}m", h, m)
            } else {
                format!("{}h", h)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_approx_eq {
        ($a:expr, $b:expr) => {
            assert!(($a - $b).abs() < 1e-5, "left: {}, right: {}", $a, $b);
        };
    }

    #[test]
    fn test_record_and_smooth() {
        let mut hist = MetricsHistory::new();
        hist.set_time(10.0);
        hist.record(1, 100.0, 50.0);

        // Initial value is perfectly accepted
        assert_approx_eq!(hist.smoothed_cpu(1), 100.0);
        assert_approx_eq!(hist.smoothed_mem(1), 50.0);

        // Next value smoothed with alpha=0.3
        hist.set_time(11.0);
        hist.record(1, 10.0, 10.0);
        // smoothed = 0.3 * 10 + 0.7 * 100 = 3 + 70 = 73
        assert_approx_eq!(hist.smoothed_cpu(1), 73.0);
    }

    #[test]
    fn test_sustained_cpu() {
        let mut hist = MetricsHistory::new();
        for i in 0..10 {
            hist.set_time(100.0 + i as f64);
            hist.record(1, 50.0, 10.0);
        }

        hist.set_time(109.0);
        assert_approx_eq!(hist.sustained_cpu(1, 5.0), 50.0);
    }

    #[test]
    fn test_memory_growth_rate() {
        let mut hist = MetricsHistory::new();
        // 10s period, +10MB
        hist.set_time(100.0);
        hist.record(1, 0.0, 100.0);
        hist.set_time(110.0);
        hist.record(1, 0.0, 110.0);

        hist.set_time(110.0);
        // dm=10, dt=10 => 1MB/s => 60MB/min
        assert_approx_eq!(hist.memory_growth_rate(1, 30.0), 60.0);
    }

    #[test]
    fn test_sparkline_resampling() {
        let mut hist = MetricsHistory::new();
        for i in 0..5 {
            hist.set_time(i as f64);
            hist.record(1, 100.0 * (i as f64) / 4.0, 0.0);
        }

        // Values are 0.0, 25.0, 50.0, 75.0, 100.0
        let spark = hist.sparkline(1, 5, "cpu");
        assert_eq!(spark, " ▂▄▆█");

        // Pad left if 10 width
        let spark10 = hist.sparkline(1, 10, "cpu");
        assert_eq!(spark10, "      ▂▄▆█");
    }

    #[test]
    fn test_sparkline_for_group() {
        let mut hist = MetricsHistory::new();
        // PID 1 has 50% flat
        for i in 0..5 {
            hist.set_time(i as f64);
            hist.record(1, 50.0, 0.0);
        }
        // PID 2 has 50% flat, but only last 3 seconds
        for i in 2..5 {
            hist.set_time(i as f64);
            hist.record(2, 50.0, 0.0);
        }

        hist.set_time(4.0);
        let spark = hist.sparkline_for_group(&[1, 2], 5);
        // For t=[0,1], sum is 50. For t=[2,3,4], sum is 100
        // max_val = 100
        // Normalized: 0.5, 0.5, 1.0, 1.0, 1.0
        assert_eq!(spark, "▄▄███");
    }

    #[test]
    fn test_expire_stale() {
        let mut hist = MetricsHistory::new();
        hist.set_time(0.0);
        hist.record(1, 10.0, 10.0);
        assert_eq!(hist.smoothed_cpu(1), 10.0);

        hist.set_time(100.0); // EXPIRY_S is 90
        hist.expire_stale();
        assert_eq!(hist.smoothed_cpu(1), 0.0);
        assert_eq!(hist.buffers.contains_key(&1), false);
    }

    #[test]
    fn test_idle_hidden_process_scores_for_background() {
        let hist = MetricsHistory::new();
        let result = hist.compute_reclaim_score(
            "TestApp",
            "TestApp",
            "",
            &[1],
            0.1,
            50.0,
            1,
            true,
            false,
            false,
            "",
            0.0,
        );
        assert!(result.score >= 15);
        assert_eq!(result.risk, "safe");
    }

    #[test]
    fn test_high_memory_process_scores_for_memory() {
        let hist = MetricsHistory::new();
        let result = hist.compute_reclaim_score(
            "BigApp",
            "BigApp",
            "",
            &[1],
            0.0,
            2500.0,
            1,
            false,
            false,
            false,
            "",
            0.0,
        );
        assert!(result.score >= 25);
    }

    #[test]
    fn test_process_storm_adds_10_points() {
        let hist = MetricsHistory::new();
        let result_storm = hist.compute_reclaim_score(
            "StormApp",
            "StormApp",
            "",
            &[1],
            0.0,
            100.0,
            15,
            false,
            false,
            false,
            "",
            0.0,
        );
        let result_calm = hist.compute_reclaim_score(
            "CalmApp",
            "CalmApp",
            "",
            &[1],
            0.0,
            100.0,
            3,
            false,
            false,
            false,
            "",
            0.0,
        );
        assert!(result_storm.score >= result_calm.score + 10);
    }

    #[test]
    fn test_high_wakeups_adds_5_points() {
        let hist = MetricsHistory::new();
        let base = hist.compute_reclaim_score(
            "A",
            "A",
            "",
            &[1],
            0.0,
            100.0,
            1,
            false,
            false,
            false,
            "",
            0.0,
        );
        let with_wakeups = hist.compute_reclaim_score(
            "A",
            "A",
            "",
            &[1],
            0.0,
            100.0,
            1,
            false,
            false,
            true,
            "",
            0.0,
        );
        assert_eq!(with_wakeups.score, base.score + 5);
    }

    #[test]
    fn test_system_process_gets_danger_risk() {
        let hist = MetricsHistory::new();
        let result = hist.compute_reclaim_score(
            "kernel_task",
            "kernel_task",
            "",
            &[1],
            5.0,
            1000.0,
            1,
            false,
            true,
            false,
            "",
            0.0,
        );
        assert_eq!(result.risk, "danger");
        assert!(result.suggested_action.contains("Review"));
    }

    #[test]
    fn test_visible_high_cpu_gets_review_risk() {
        let hist = MetricsHistory::new();
        let result = hist.compute_reclaim_score(
            "ActiveApp",
            "ActiveApp",
            "",
            &[1],
            30.0,
            200.0,
            1,
            false,
            false,
            false,
            "",
            0.0,
        );
        assert_eq!(result.risk, "review");
    }

    #[test]
    fn test_score_never_exceeds_100() {
        let mut hist = MetricsHistory::new();
        for i in 0..30 {
            hist.set_time(i as f64);
            hist.record(1, 100.0, 5000.0);
        }
        let result = hist.compute_reclaim_score(
            "Monster",
            "Monster",
            "",
            &[1],
            100.0,
            5000.0,
            50,
            true,
            false,
            true,
            "",
            0.0,
        );
        assert!(result.score <= 100);
    }

    #[test]
    fn test_reason_generation() {
        let hist = MetricsHistory::new();

        // Hidden with long launch age
        let r1 = hist.compute_reclaim_score(
            "H",
            "H",
            "",
            &[1],
            0.1,
            50.0,
            1,
            true,
            false,
            false,
            "",
            600.0,
        );
        assert!(r1.reason.to_lowercase().contains("hidden"));

        // High memory
        let r2 = hist.compute_reclaim_score(
            "M",
            "M",
            "",
            &[1],
            1.0,
            2048.0,
            1,
            false,
            false,
            false,
            "",
            0.0,
        );
        assert!(r2.reason.contains("G") || r2.reason.contains("resident"));

        // Storm
        let r3 = hist.compute_reclaim_score(
            "S",
            "S",
            "",
            &[1],
            1.0,
            100.0,
            25,
            false,
            false,
            false,
            "",
            0.0,
        );
        assert!(r3.reason.contains("25") || r3.reason.contains("child"));
    }
}
