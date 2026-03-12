use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use tabled::Tabled;


/// Fully Resolved, display-ready information for one process or thread. 
/// This is the data contract detween 'proc' and 'display'
#[derive(Tabled, Debug)]
pub struct ProcessInfo {
    #[tabled(rename = "PID")]
    pub pid: String,
    #[tabled(rename = "PPID")]
    pub ppid: String,
    #[tabled(rename = "NAME")]
    pub name: String,
    #[tabled(rename = "STATE")]
    pub state: String,
    #[tabled(rename = "%CPU")]
    pub cpu_percent: String,
    #[tabled(rename = "%MEM")]
    pub mem_percent: String,
    #[tabled(rename = "RSS KB")]
    pub rss_kb: String,
    #[tabled(rename = "VSZ KB")]
    pub vsz_kb: String,
    #[tabled(rename = "THREADS")]
    pub threads: String,
    #[tabled(rename = "FDS")]
    pub file_descriptors: String,
    #[tabled(rename = "CPU_TIME")]
    pub cpu_time: String,
    #[tabled(rename = "UPTIME")]
    pub uptime: String,
}

/// Raw field parsed from `/proc/{pid}/stat`
#[derive(Debug, Clone)]
struct ProcStat {
    comm: String,
    state: char,
    ppid: i32,
    utime: u64,
    stime: u64,
    num_threads: i32,
    starttime: u64,
}

/// Raw field parsed from `/proc/{pid}/statm`
#[derive(Debug, Clone)]
struct ProcStatm {
    size: u64,     // total program size in pages
    resident: u64, // resident set size in pages
}

/// System-wide constants.
#[derive(Debug, Clone)]
pub struct SystemInfo {
    total_memory_kb: u64,
    boot_time: u64,
    cpu_count: u32,
    page_size: u64,
    clock_tck: u64, // Cached SC_CLK_TCK
}

/// Per-sample CPU accounting state. Pass a single instance through the
/// monitoring loop.
pub struct CpuStats {
    prev_total_time: HashMap<i32, u64>,
    prev_system_time: u64,
    system_info: SystemInfo,
}

impl CpuStats {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let system_info = get_system_info()?;
        Ok(CpuStats {
            prev_total_time: HashMap::new(),
            prev_system_time: get_system_cpu_time()?,
            system_info,
        })
    }
}

/// Collect fresh `ProcessInfo` for every PID in `pids`
/// Optionally expands each PID to include its threads (`show_threads`)
pub fn get_process_data_realtime(
    pids: &[i32],
    cpu_stats: &mut CpuStats,
    show_threads: bool,
) -> Result<Vec<ProcessInfo>, Box<dyn std::error::Error>> {
    let mut all_processes = Vec::new();
    let current_system_time = get_system_cpu_time()?;

    for &pid in pids {
        match get_single_process_info(pid, cpu_stats, current_system_time) {
            Ok(mut process_info) => {
                all_processes.append(&mut process_info);

                if show_threads {
                    if let Ok(thread_info) = get_thread_info(pid, cpu_stats, current_system_time) {
                        all_processes.extend(thread_info)
                    }
                }
            }
            Err(e) => eprintln!(" Warning: Could not get info for PID {}: {}", pid, e),
        }
    }
    
    // Update system time AFTER all PIDs are processed so every PID uses
    // same baseline on the next iteration - even if some PIDs errors out.
    cpu_stats.prev_system_time = current_system_time;
    Ok(all_processes)
} 

/// Warm up `cpu_stats` by recording the current CPU ticks for each PID
/// without producing optput.
pub fn prime_cpu_baseline(
    pids: &[i32],
    cpu_stats: &mut CpuStats
) -> Result<(), Box<dyn std::error::Error>> {
    let current_system_time = get_system_cpu_time()?;
    for &pid in pids {
        if let Ok(stat) = read_proc_stat(pid) {
            cpu_stats
                .prev_total_time
                .insert(pid, stat.utime + stat.stime);
        }
    }
    cpu_stats.prev_system_time = current_system_time;
    Ok(())
}

fn get_single_process_info(
    pid: i32,
    cpu_stats: &mut CpuStats,
    current_system_time: u64,
) -> Result<Vec<ProcessInfo>, Box<dyn std::error::Error>> {
    let stat = read_proc_stat(pid)?;
    let statm = read_proc_statm(pid)?;
    let fd_count = count_file_descriptors(pid)?;

    let current_total_time = stat.utime + stat.stime;
    let cpu_percent = if let Some(&prev_total_time) = cpu_stats.prev_total_time.get(&pid) {
        let time_diff = current_total_time.saturating_sub(prev_total_time);
        let system_time_diff = current_system_time.saturating_sub(cpu_stats.prev_system_time);

        if system_time_diff > 0 {
            (time_diff as f64 / system_time_diff as f64)
                * 100.0
                * cpu_stats.system_info.cpu_count as f64
        } else {
            0.0
        }
    } else {
        0.0
    };

    cpu_stats.prev_total_time.insert(pid, current_total_time);

    // calculate memory percentage
    let rss_kb = statm.resident * cpu_stats.system_info.page_size / 1024;
    let vsz_kb = statm.size * cpu_stats.system_info.page_size / 1024;
    let mem_percent = (rss_kb as f64 / cpu_stats.system_info.total_memory_kb as f64) * 100.0;

    // calculate uptime using cached clock_tck
    let clock_tck = cpu_stats.system_info.clock_tck;
    let boot_time = cpu_stats.system_info.boot_time;
    let process_start_time = boot_time
        + (stat.starttime / clock_tck);
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let uptime_secs = current_time.saturating_sub(process_start_time);

    let total_cpu_time_secs = (stat.utime + stat.stime) / clock_tck;
    let cpu_time_formatted = format_time(total_cpu_time_secs);
    let uptime_formatted = format_time(uptime_secs);

    let process_info = ProcessInfo {
        pid: pid.to_string(),
        ppid: stat.ppid.to_string(),
        name: stat.comm,
        state: format!("{}", stat.state),
        cpu_percent: format!("{:.1}", cpu_percent),
        mem_percent: format!("{:.1}", mem_percent),
        rss_kb: format!("{}", rss_kb),
        vsz_kb: format!("{}", vsz_kb),
        threads: stat.num_threads.to_string(),
        file_descriptors: fd_count.to_string(),
        cpu_time: cpu_time_formatted,
        uptime: uptime_formatted,
    };

    Ok(vec![process_info])
}

fn get_thread_info(
    pid: i32,
    cpu_stats: &mut CpuStats,
    current_system_time: u64,
) -> Result<Vec<ProcessInfo>, Box<dyn std::error::Error>> {
    let task_dir = format!("/proc/{}/task", pid);
    let mut thread_info = Vec::new();

    if let Ok(entries) = fs::read_dir(&task_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                if let Ok(tid) = entry.file_name().to_string_lossy().parse::<i32>() {
                    if tid != pid {
                        if let Ok(mut info) =
                            get_single_process_info(tid, cpu_stats, current_system_time)
                        {
                            for process in &mut info {
                                process.name = format!("|- {}", process.name);
                            }
                            thread_info.extend(info);
                        }
                    }
                }
            }
        }
    }
    Ok(thread_info)
}

fn read_proc_stat(pid: i32) -> Result<ProcStat, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(format!("/proc/{}/stat", pid))?;
    
    // Process name can contain spaces and parentheses (e.g "tmux: server"),
    // so splitting by whitespace shifts all fields offsets. We must locate
    // the LAST ')' to correctly identify where the comm field ends.
    
    let comm_start = content
        .find("(")
        .ok_or_else(|| format!("No '(' in /proc/{}/stat", pid))?;
    let comm_end = content
        .find(")")
        .ok_or_else(|| format!("No ')' in /proc/{}/stat", pid))?;
    
    let comm = content[comm_start+1..comm_end].to_string();
    
    // Fields after ')' are space seperated: their local indices ( per the 
    // kernel docs) start at 3, but our slice starts at 0:
    //  rest[0] = field 3 (state)
    //  rest[1] = field 4 (ppid)
    //  rest[11] = field 14 (utime)
    //  rest[12] = field 15 (stime)
    //  rest[17] = field 20 (num_threads) 
    //  rest[19] = field 22 (starttime)
    
    let rest: Vec<&str> = content[comm_end+1..].split_whitespace().collect();
    
    if rest.len() < 20 {
        return Err(format!(
            "Insufficient fields after comm in /proc/{}/stat (got {}, need 20)",
            pid, 
            rest.len()
        )
        .into());
    }
   
    Ok(ProcStat {
        comm,
        state: rest[0].chars().next().unwrap_or('?'),
        ppid: rest[3].parse()?,
        utime: rest[11].parse()?,
        stime: rest[12].parse()?,
        num_threads: rest[17].parse()?,
        starttime: rest[19].parse()?,
    })
}

fn read_proc_statm(pid: i32) -> Result<ProcStatm, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(format!("/proc/{}/statm", pid))?;
    let fields: Vec<&str> = content.trim().split_whitespace().collect();

    if fields.len() < 3 {
        return Err("Insufficient fields in /proc/pid/statm".into());
    }

    Ok(ProcStatm {
        size: fields[0].parse()?,
        resident: fields[1].parse()?,
    })
}

fn count_file_descriptors(pid: i32) -> Result<usize, Box<dyn std::error::Error>> {
    let fd_dir = format!("/proc/{}/fd", pid);
    match fs::read_dir(&fd_dir) {
        Ok(entries) => Ok(entries.count()),
        Err(_) => Ok(0), // process might not have permissions or might not started
    }
}

fn get_system_info() -> Result<SystemInfo, Box<dyn std::error::Error>> {
    let meminfo = fs::read_to_string("/proc/meminfo")?;
    let total_memory_kb = meminfo
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    // Get boot time
    let stat = fs::read_to_string("/proc/stat")?;
    let boot_time = stat
        .lines()
        .find(|line| line.starts_with("btime "))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let cpu_count = std::thread::available_parallelism()?.get() as u32;
    let page_size = sysconf::sysconf(sysconf::SysconfVariable::ScPagesize).unwrap() as u64;
    let clock_tck = sysconf::sysconf(sysconf::SysconfVariable::ScClkTck).unwrap() as u64;

    Ok(SystemInfo {
        total_memory_kb,
        boot_time,
        cpu_count,
        page_size,
        clock_tck
    })
}

fn get_system_cpu_time() -> Result<u64, Box<dyn std::error::Error>> {
    let stat = fs::read_to_string("/proc/stat")?;
    let cpu_time = stat.lines().next().ok_or("No CPU line in /proc/stat")?;
    let fields: Vec<&str> = cpu_time.split_whitespace().collect();

    if fields.len() < 8 {
        return Err("Insufficient CPU fields in /proc/stat".into());
    }

    let total: u64 = fields[1..8]
        .iter()
        .filter_map(|s| s.parse::<u64>().ok())
        .sum();
    Ok(total)
}

/// Format a duration in seconds as "Xh Ym Xs", "Ym Zs", "Zs"
fn format_time(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}h{}m{}s", hours, minutes, secs)
    } else if minutes > 0 {
        format!("{}m{}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}