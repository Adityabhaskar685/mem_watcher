use clap::Parser;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tabled::Tabled;
use tabled::settings::object::Rows;
use tabled::settings::{Color, Style};

#[derive(Parser)]
#[command(name = "mem watcher")]
#[command(about = "Process details watcher using /proc filesystem")]
#[command(version = "1.0")]
struct Args {
    #[arg(
        short = 'p',
        long = "process_id",
        required = true,
        help = "Process ID seperated by space"
    )]
    pid: Vec<i32>,

    #[arg(
        short = 'd',
        long = "duration",
        help = "Monitor duration in seconds (default: single snapshot)"
    )]
    duration: Option<u64>,

    #[arg(
        short = 'i',
        long = "interval",
        default_value = "2",
        help = "Update interval in seconds (default: 2)"
    )]
    interval: u64,

    #[arg(long = "no-clear", help = "Don't clear screen between updates")]
    no_clear: bool,

    #[arg(long = "show-threads", help = "Show individual threads")]
    show_threads: bool,
}

#[derive(Tabled, Debug, Deserialize)]
struct ProcessInfo {
    #[tabled(rename = "PID")]
    pid: String,
    #[tabled(rename = "PPID")]
    ppid: String,
    #[tabled(rename = "NAME")]
    name: String,
    #[tabled(rename = "STATE")]
    state: String,
    #[tabled(rename = "%CPU")]
    cpu_percent: String,
    #[tabled(rename = "%MEM")]
    mem_percent: String,
    #[tabled(rename = "RSS KB")]
    rss_kb: String,
    #[tabled(rename = "VSZ KB")]
    vsz_kb: String,
    #[tabled(rename = "THREADS")]
    threads: String,
    #[tabled(rename = "FDS")]
    file_descriptors: String,
    #[tabled(rename = "CPU_TIME")]
    cpu_time: String,
    #[tabled(rename = "UPTIME")]
    uptime: String,
}

#[derive(Debug, Clone)]
struct ProcStat {
    pid: i32,
    comm: String,
    state: char,
    ppid: i32,
    utime: u64,
    stime: u64,
    cutime: u64,
    cstime: u64,
    num_threads: i32,
    starttime: u64,
}

#[derive(Debug, Clone)]
struct ProcStatm {
    size: u64,     // Total program size (pages)
    resident: u64, // Residend set size (pages)
    shared: u64,   // Shared pages
}

#[derive(Debug, Clone)]
struct SystemInfo {
    total_memory_kb: u64,
    boot_time: u64,
    cpu_count: u32,
    page_size: u64,
}

struct CpuStats {
    prev_total_time: HashMap<i32, u64>,
    prev_system_time: u64,
    system_info: SystemInfo,
}

impl CpuStats {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let system_info = get_system_info()?;
        Ok(CpuStats {
            prev_total_time: HashMap::new(),
            prev_system_time: get_system_cpu_time()?,
            system_info,
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.duration {
        Some(duration) => monitor_continously(&args, duration)?,
        None => display_snapshot(&args)?,
    }

    Ok(())
}

fn monitor_continously(args: &Args, duration_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();
    let duration = Duration::from_secs(duration_secs);
    let interval = Duration::from_secs(args.interval);

    let mut cpu_stats = CpuStats::new()?;
    println!(
        "🔍 Monitoring processes {:?} for {} seconds (interval: {}s)",
        args.pid, duration_secs, args.interval
    );
    println!("Press Ctrl+C to stop monitoring early");
    println!("{}", "=".repeat(80));

    while start_time.elapsed() < duration {
        let iteration_start = Instant::now();

        if !args.no_clear {
            // clear the screen
            print!("\x1B[2J\x1B[1;1H");
        }

        let elapsed = start_time.elapsed().as_secs();
        let remaining = duration_secs.saturating_sub(elapsed);

        println!(
            "⏱️  Elapsed: {}s | Remaining: {}s | Updated: {}",
            elapsed,
            remaining,
            chrono::Local::now().format("%H:%M:%S")
        );
        println!("{}", "-".repeat(80));

        match get_process_data_realtime(&args.pid, &mut cpu_stats, args.show_threads) {
            Ok(processes) => display_table(&processes),
            Err(e) => eprintln!("❌ Error collecting process data: {}", e),
        }
        let interval_time = iteration_start.elapsed();
        if interval_time < interval {
            thread::sleep(interval - interval_time);
        }
    }

    println!("\n✅ Monitoring completed after {} seconds", duration_secs);
    Ok(())
}

fn display_snapshot(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    println!("Process snapshot for PIDs {:?}", args.pid);
    println!("{}", "=".repeat(80));

    let mut cpu_stats = CpuStats::new()?;

    let processes = get_process_data_realtime(&args.pid, &mut cpu_stats, args.show_threads)?;
    display_table(&processes);

    Ok(())
}

fn get_process_data_realtime(
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
    cpu_stats.prev_system_time = current_system_time;
    Ok(all_processes)
}

fn get_single_process_info(
    pid: i32,
    cpu_stats: &mut CpuStats,
    current_system_time: u64,
) -> Result<Vec<ProcessInfo>, Box<dyn std::error::Error>> {
    let stat = read_proc_stat(pid)?;
    let statm = read_proc_statm(pid)?;
    let fd_count = count_file_descriptors(pid)?;

    let current_total_time = stat.utime + stat.stime + stat.cutime + stat.cstime;
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

    // calculate uptime
    let boot_time = cpu_stats.system_info.boot_time;
    let process_start_time = boot_time
        + (stat.starttime / sysconf::sysconf(sysconf::SysconfVariable::ScClkTck).unwrap() as u64);
    let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let uptime_secs = current_time.saturating_sub(process_start_time);

    // Format CPU time
    let total_cpu_time_secs = (stat.utime + stat.stime)
        / sysconf::sysconf(sysconf::SysconfVariable::ScClkTck).unwrap() as u64;
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
    let fields: Vec<&str> = content.trim().split_whitespace().collect();

    if fields.len() < 22 {
        return Err("Insufficient fields in /proc/pid/stat".into());
    }

    Ok(ProcStat {
        pid: fields[0].parse()?,
        comm: fields[1].trim_matches(|c| c == '(' || c == ')').to_string(),
        state: fields[2].chars().next().unwrap_or('?'),
        ppid: fields[3].parse()?,
        utime: fields[13].parse()?,
        stime: fields[14].parse()?,
        cutime: fields[15].parse()?,
        cstime: fields[16].parse()?,
        num_threads: fields[19].parse()?,
        starttime: fields[21].parse()?,
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
        shared: fields[2].parse()?,
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

    Ok(SystemInfo {
        total_memory_kb,
        boot_time,
        cpu_count,
        page_size,
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

fn display_table(processes: &[ProcessInfo]) {
    if processes.is_empty() {
        println!(" No processes found");
        return;
    }

    let mut table = tabled::Table::new(processes);
    table.with(Style::rounded());
    table.modify(Rows::first(), Color::FG_GREEN);

    for (i, process) in processes.iter().enumerate() {
        let row_index = i + 1;

        if let Ok(cpu_value) = process.cpu_percent.parse::<f64>() {
            if cpu_value >= 50.0 {
                table.modify(Rows::single(row_index), Color::FG_RED);
            }
        }

        if let Ok(mem_val) = process.mem_percent.parse::<f64>() {
            if mem_val >= 10.0 {
                table.modify(Rows::single(row_index), Color::FG_RED);
            }
        }
    }

    println!("{}", table);

    let total_processes = processes.len();
    let avg_cpu: f64 = processes
        .iter()
        .filter_map(|p| p.cpu_percent.parse::<f64>().ok())
        .sum::<f64>()
        / total_processes as f64;
    let avg_mem: f64 = processes
        .iter()
        .filter_map(|p| p.mem_percent.parse::<f64>().ok())
        .sum::<f64>()
        / total_processes as f64;

    let total_rss: u64 = processes
        .iter()
        .filter_map(|p| p.rss_kb.parse::<u64>().ok())
        .sum();
    let total_fds: u64 = processes
        .iter()
        .filter_map(|p| p.file_descriptors.parse::<u64>().ok())
        .sum();

    println!(
        "\n📈 Summary: {} proc | CPU: {:.1}% | MEM: {:.1}% | RSS: {} KB | FDs: {}",
        total_processes, avg_cpu, avg_mem, total_rss, total_fds
    );
}
