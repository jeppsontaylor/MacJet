use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, Default)]
pub struct EnergyInfo {
    pub pid: u32,
    pub name: SmolStr,
    pub energy_impact: f64,
    pub cpu_ms_per_s: f64,
    pub wakeups_per_s: f64,
    pub gpu_ms_per_s: f64,
    pub bytes_read_per_s: f64,
    pub bytes_written_per_s: f64,
    pub packets_in_per_s: f64,
    pub packets_out_per_s: f64,
    pub coalition: SmolStr,
}

#[derive(Debug, Clone)]
pub struct ThermalInfo {
    pub cpu_die_temp: f64,
    pub gpu_die_temp: f64,
    pub fan_speed_rpm: u32,
    pub fan_speed_max: u32,
    pub thermal_pressure: SmolStr,
    pub gpu_active_percent: f64,
}

impl Default for ThermalInfo {
    fn default() -> Self {
        Self {
            cpu_die_temp: 0.0,
            gpu_die_temp: 0.0,
            fan_speed_rpm: 0,
            fan_speed_max: 0,
            thermal_pressure: SmolStr::new("nominal"),
            gpu_active_percent: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EnergySnapshot {
    pub processes: FxHashMap<u32, EnergyInfo>,
    pub coalitions: FxHashMap<SmolStr, Vec<EnergyInfo>>,
    pub thermal: ThermalInfo,
    pub timestamp: f64,
}

pub struct EnergyCollector {
    process: Option<Child>,
    pub latest: Arc<Mutex<EnergySnapshot>>,
    has_sudo: bool,
    running: bool,
}

impl Default for EnergyCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyCollector {
    pub fn new() -> Self {
        Self {
            process: None,
            latest: Arc::new(Mutex::new(EnergySnapshot::default())),
            has_sudo: false,
            running: false,
        }
    }

    pub fn snapshot(&self) -> EnergySnapshot {
        let lock = self.latest.lock().unwrap();
        lock.clone()
    }

    pub fn has_sudo(&self) -> bool {
        self.has_sudo
    }

    pub fn start(&mut self) -> bool {
        // Safe cross-platform way to get root vs unsafe libc
        let is_root = if let Ok(out) = std::process::Command::new("id").arg("-u").output() {
            String::from_utf8_lossy(&out.stdout).trim() == "0"
        } else {
            false
        };

        if !is_root {
            self.has_sudo = false;
            return false;
        }

        self.has_sudo = true;
        self.running = true;

        match Command::new("/usr/bin/powermetrics")
            .arg("--format")
            .arg("plist")
            .arg("--samplers")
            .arg("tasks,smc,gpu_power")
            .arg("-i")
            .arg("2000") // 2 second interval
            .arg("--show-process-energy")
            .arg("--show-process-gpu")
            .arg("--show-process-coalition")
            .arg("--show-process-netstats")
            .arg("--show-process-io")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                let stdout = child.stdout.take().unwrap();
                let latest_ref = Arc::clone(&self.latest);

                // Spawn background reader thread
                thread::spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    let mut buffer = Vec::new();
                    let mut line = String::new();

                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        buffer.extend_from_slice(line.as_bytes());
                        if line.trim() == "</plist>" {
                            Self::parse_plist_static(&buffer, &latest_ref);
                            buffer.clear();
                        }
                        line.clear();
                    }
                });

                self.process = Some(child);
                true
            }
            Err(_) => {
                self.has_sudo = false;
                false
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn parse_plist(&mut self, data: &[u8]) {
        Self::parse_plist_static(data, &self.latest);
    }

    fn parse_plist_static(data: &[u8], latest_mutex: &Arc<Mutex<EnergySnapshot>>) {
        let mut snapshot = EnergySnapshot::default();

        let Ok(value) = plist::from_bytes::<plist::Value>(data) else {
            return;
        };

        if let Some(dict) = value.as_dictionary() {
            // Process tasks
            if let Some(tasks) = extract_array(dict, "tasks") {
                for task in tasks {
                    if let Some(t_dict) = task.as_dictionary() {
                        let pid = extract_u64_or_0(t_dict, "pid");
                        if pid == 0 {
                            continue;
                        } // ignore kernel_task

                        let info = EnergyInfo {
                            pid: pid as u32,
                            name: SmolStr::new(extract_string(t_dict, "name")),
                            energy_impact: extract_f64_or_0(t_dict, "energy_impact"),
                            cpu_ms_per_s: extract_f64_or_0(t_dict, "cpu_ms_per_s"),
                            wakeups_per_s: extract_f64_or_0(t_dict, "wakeups_per_s"),
                            gpu_ms_per_s: extract_f64_or_0(t_dict, "gpu_ms_per_s"),
                            bytes_read_per_s: extract_f64_or_0(t_dict, "bytes_read_per_s"),
                            bytes_written_per_s: extract_f64_or_0(t_dict, "bytes_written_per_s"),
                            packets_in_per_s: extract_f64_or_0(t_dict, "packets_in_per_s"),
                            packets_out_per_s: extract_f64_or_0(t_dict, "packets_out_per_s"),
                            coalition: SmolStr::new(extract_string(t_dict, "coalition")),
                        };
                        snapshot.processes.insert(pid as u32, info);
                    }
                }
            }

            // Process thermal / smc
            if let Some(smc) = extract_dict(dict, "smc") {
                if let Some(fan) = extract_array(smc, "fan") {
                    if let Some(first_fan) = fan.first().and_then(|v| v.as_dictionary()) {
                        snapshot.thermal.fan_speed_rpm =
                            extract_u64_or_0(first_fan, "speed") as u32;
                        snapshot.thermal.fan_speed_max =
                            extract_u64_or_0(first_fan, "max_speed") as u32;
                    }
                }
                snapshot.thermal.cpu_die_temp = extract_f64_or_0(smc, "cpu_die_temp");
            }

            if let Some(proc) = extract_dict(dict, "processor") {
                snapshot.thermal.thermal_pressure =
                    SmolStr::new(extract_string(proc, "thermal_pressure"));
            }

            if let Some(gpu) = extract_dict(dict, "gpu") {
                snapshot.thermal.gpu_active_percent = extract_f64_or_0(gpu, "gpu_active_percent");
            }
        }

        let mut target = latest_mutex.lock().unwrap();
        *target = snapshot;
    }

    pub fn get_energy_label(&self, pid: u32) -> &'static str {
        let snap = self.snapshot();
        if let Some(info) = snap.processes.get(&pid) {
            if info.energy_impact >= 50.0 {
                "HIGH"
            } else if info.energy_impact >= 20.0 {
                "MED"
            } else if info.energy_impact >= 5.0 {
                "LOW"
            } else {
                ""
            }
        } else {
            ""
        }
    }
}

fn extract_dict<'a>(dict: &'a plist::Dictionary, key: &str) -> Option<&'a plist::Dictionary> {
    dict.get(key).and_then(|v| v.as_dictionary())
}

fn extract_array<'a>(dict: &'a plist::Dictionary, key: &str) -> Option<&'a Vec<plist::Value>> {
    dict.get(key).and_then(|v| v.as_array())
}

fn extract_f64_or_0(dict: &plist::Dictionary, key: &str) -> f64 {
    dict.get(key)
        .and_then(|v| {
            v.as_real()
                .or_else(|| v.as_unsigned_integer().map(|i| i as f64))
        })
        .unwrap_or(0.0)
}

fn extract_u64_or_0(dict: &plist::Dictionary, key: &str) -> u64 {
    dict.get(key)
        .and_then(|v| v.as_unsigned_integer())
        .unwrap_or(0)
}

fn extract_string(dict: &plist::Dictionary, key: &str) -> String {
    dict.get(key)
        .and_then(|v| v.as_string())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::Value;

    fn make_plist_bytes(dict: plist::Dictionary) -> Vec<u8> {
        let mut buf = Vec::new();
        plist::to_writer_xml(&mut buf, &Value::Dictionary(dict)).unwrap();
        buf
    }

    #[test]
    fn test_parses_tasks() {
        let mut ec = EnergyCollector::new();

        // Build {"tasks": [{"pid": 100, "name": "Chrome", "energy_impact": 42.5, "wakeups_per_s": 80.0}, {"pid": 200, "name": "node", "energy_impact": 15.0}]}
        let mut dict = plist::Dictionary::new();
        let mut tasks = Vec::new();

        let mut c1 = plist::Dictionary::new();
        c1.insert("pid".into(), Value::Integer(100.into()));
        c1.insert("name".into(), Value::String("Chrome".into()));
        c1.insert("energy_impact".into(), Value::Real(42.5));
        c1.insert("wakeups_per_s".into(), Value::Real(80.0));
        tasks.push(Value::Dictionary(c1));

        let mut c2 = plist::Dictionary::new();
        c2.insert("pid".into(), Value::Integer(200.into()));
        c2.insert("name".into(), Value::String("node".into()));
        c2.insert("energy_impact".into(), Value::Real(15.0));
        tasks.push(Value::Dictionary(c2));

        dict.insert("tasks".into(), Value::Array(tasks));

        let bytes = make_plist_bytes(dict);
        ec.parse_plist(&bytes);

        ec.parse_plist(&bytes);

        let snap = ec.snapshot();
        assert!(snap.processes.contains_key(&100));
        assert_eq!(snap.processes.get(&100).unwrap().energy_impact, 42.5);
        assert_eq!(snap.processes.get(&100).unwrap().wakeups_per_s, 80.0);
        assert!(snap.processes.contains_key(&200));
    }

    #[test]
    fn test_parses_thermal_pressure() {
        let mut ec = EnergyCollector::new();
        let mut dict = plist::Dictionary::new();
        let mut proc = plist::Dictionary::new();
        proc.insert("thermal_pressure".into(), Value::String("heavy".into()));
        dict.insert("processor".into(), Value::Dictionary(proc));

        ec.parse_plist(&make_plist_bytes(dict));
        assert_eq!(ec.snapshot().thermal.thermal_pressure, "heavy");
    }

    #[test]
    fn test_parses_fan_speed() {
        let mut ec = EnergyCollector::new();
        let mut dict = plist::Dictionary::new();
        let mut smc = plist::Dictionary::new();
        let mut fan = plist::Dictionary::new();
        fan.insert("speed".into(), Value::Integer(3200.into()));
        fan.insert("max_speed".into(), Value::Integer(6000.into()));
        smc.insert("fan".into(), Value::Array(vec![Value::Dictionary(fan)]));
        dict.insert("smc".into(), Value::Dictionary(smc));

        ec.parse_plist(&make_plist_bytes(dict));
        assert_eq!(ec.snapshot().thermal.fan_speed_rpm, 3200);
        assert_eq!(ec.snapshot().thermal.fan_speed_max, 6000);
    }

    #[test]
    fn test_parses_cpu_temperature() {
        let mut ec = EnergyCollector::new();
        let mut dict = plist::Dictionary::new();
        let mut smc = plist::Dictionary::new();
        smc.insert("cpu_die_temp".into(), Value::Real(78.5));
        dict.insert("smc".into(), Value::Dictionary(smc));

        ec.parse_plist(&make_plist_bytes(dict));
        assert_eq!(ec.snapshot().thermal.cpu_die_temp, 78.5);
    }

    #[test]
    fn test_parses_gpu_active_percent() {
        let mut ec = EnergyCollector::new();
        let mut dict = plist::Dictionary::new();
        let mut gpu = plist::Dictionary::new();
        gpu.insert("gpu_active_percent".into(), Value::Real(45.0));
        dict.insert("gpu".into(), Value::Dictionary(gpu));

        ec.parse_plist(&make_plist_bytes(dict));
        assert_eq!(ec.snapshot().thermal.gpu_active_percent, 45.0);
    }

    #[test]
    fn test_skips_pid_zero() {
        let mut ec = EnergyCollector::new();
        let mut dict = plist::Dictionary::new();
        let mut tasks = Vec::new();
        let mut c1 = plist::Dictionary::new();
        c1.insert("pid".into(), Value::Integer(0.into()));
        c1.insert("name".into(), Value::String("kernel_task".into()));
        tasks.push(Value::Dictionary(c1));
        dict.insert("tasks".into(), Value::Array(tasks));

        ec.parse_plist(&make_plist_bytes(dict));
        assert!(!ec.snapshot().processes.contains_key(&0));
    }

    #[test]
    fn test_handles_empty_plist() {
        let mut ec = EnergyCollector::new();
        let dict = plist::Dictionary::new();
        ec.parse_plist(&make_plist_bytes(dict));
        assert_eq!(ec.snapshot().processes.len(), 0);
    }

    #[test]
    fn test_handles_malformed_data() {
        let mut ec = EnergyCollector::new();
        ec.parse_plist(b"not valid plist data");
        // Should not panic
        assert_eq!(ec.snapshot().processes.len(), 0);
    }

    #[test]
    fn test_energy_labels() {
        let mut ec = EnergyCollector::new();

        let mut snapshot = EnergySnapshot::default();
        let mut hot = EnergyInfo::default();
        hot.pid = 1;
        hot.energy_impact = 60.0;
        snapshot.processes.insert(1, hot);

        let mut warm = EnergyInfo::default();
        warm.pid = 2;
        warm.energy_impact = 30.0;
        snapshot.processes.insert(2, warm);

        let mut cool = EnergyInfo::default();
        cool.pid = 3;
        cool.energy_impact = 10.0;
        snapshot.processes.insert(3, cool);

        let mut idle = EnergyInfo::default();
        idle.pid = 4;
        idle.energy_impact = 2.0;
        snapshot.processes.insert(4, idle);

        *ec.latest.lock().unwrap() = snapshot;

        assert_eq!(ec.get_energy_label(1), "HIGH");
        assert_eq!(ec.get_energy_label(2), "MED");
        assert_eq!(ec.get_energy_label(3), "LOW");
        assert_eq!(ec.get_energy_label(4), "");
        assert_eq!(ec.get_energy_label(9999), "");
    }
}
