use clap::Parser;
use serde::Deserialize;
use std::os::unix::process;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tabled::Tabled;
use tabled::settings::object::Rows;
use tabled::settings::{Color, Style};

#[derive(Parser)]
#[command(name = "mem watcher")]
#[command(about = "Process details watcher")]
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
        help = "Update intervval in seconds (defaults: 2)"
    )]
    interval: u64,

    #[arg(long = "no-clear", help = "Don't clear screen between updates")]
    no_clear: bool,
}

#[derive(Debug, Deserialize)]
struct Processes {
    process_info: Vec<Vec<ProcessInfo>>,
}

#[derive(Tabled, Debug, Deserialize)]
struct ProcessInfo {
    #[tabled(rename = "PID")]
    pid: String,
    #[tabled(rename = "PPID")]
    ppid: String,
    #[tabled(rename = "USER")]
    user: String,
    #[tabled(rename = "UID")]
    uid: String,
    #[tabled(rename = "GID")]
    gid: String,
    #[tabled(rename = "COMMAND")]
    comm: String,
    #[tabled(rename = "CMD")]
    cmd: String,
    #[tabled(rename = "%CPU")]
    cpu: String,
    #[tabled(rename = "%MEM")]
    mem: String,
    #[tabled(rename = "VSZ")]
    vsz: String,
    #[tabled(rename = "RSS")]
    rss: String,
    #[tabled(rename = "TTY")]
    tty: String,
    #[tabled(rename = "STAT")]
    stat: String,
    #[tabled(rename = "START")]
    start: String,
    #[tabled(rename = "TIME")]
    time: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match args.duration {
        Some(duration) => monitor_continiously(&args, duration)?,
        None => display_snapshot(&args)?,
    }

    Ok(())
}

fn monitor_continiously(args: &Args, duration_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    let start_time = Instant::now();
    let duration = Duration::from_secs(duration_secs);
    let interval = Duration::from_secs(args.interval);

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

        match get_process_data(&args.pid) {
            Ok(processes) => display_table(&processes),
            Err(e) => eprintln!("❌ Error collectiong process data: {}", e),
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
    println!("Process shapshot for PIDs {:?}", args.pid);
    println!("{}", "=".repeat(80));

    let processes = get_process_data(&args.pid)?;
    display_table(&processes);

    Ok(())
}

fn get_process_data(pids: &[i32]) -> Result<Vec<ProcessInfo>, Box<dyn std::error::Error>> {
    let mut all_processes = Vec::new();

    for &pid in pids {
        let output = Command::new("ps")
            .arg("-o")
            .arg("pid,ppid,user,uid,gid,comm,cmd,%cpu,%mem,vsz,rss,tty,stat,start,time")
            .arg("-p")
            .arg(pid.to_string())
            .output()?;

        if !output.status.success() {
            let strerr = String::from_utf8(output.stderr)?;
            eprintln!("Warning: Process {} not found - {}", pid, strerr.trim());
            continue;
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut process_info = parse_ps_output(&stdout);
        if process_info.is_empty() {
            eprintln!("Warning: No process data found for PID: {}", pid);
            continue;
        }
        all_processes.append(&mut process_info);
    }

    Ok(all_processes)
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

        if let Ok(cpu_value) = process.cpu.parse::<f64>() {
            if cpu_value >= 5.0 {
                table.modify(Rows::single(row_index), Color::FG_YELLOW);
            }

            if cpu_value >= 20.0 {
                table.modify(Rows::single(row_index), Color::FG_RED);
            }
        }
    }

    println!("{}", table);

    let total_processes = processes.len();
    let avg_cpu: f64 = processes
        .iter()
        .filter_map(|p| p.cpu.parse::<f64>().ok())
        .sum::<f64>()
        / total_processes as f64;
    let avg_mem: f64 = processes
        .iter()
        .filter_map(|p| p.mem.parse::<f64>().ok())
        .sum::<f64>()
        / total_processes as f64;

    println!(
        "Summary: {} processes | Avg CPU: {:.1}% | Avg MEM: {:.1}%)",
        total_processes, avg_cpu, avg_mem
    );
}

fn parse_ps_output(output: &str) -> Vec<ProcessInfo> {
    let lines: Vec<&str> = output.lines().collect();

    if lines.len() < 2 {
        return Vec::new();
    }

    let mut processes = Vec::new();

    for line in lines.iter().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Use a more reliable parsing method with regex-like field extraction
        // Expected format: PID PPID USER UID GID COMMAND CMD %CPU %MEM VSZ RSS TT STAT STARTED TIME
        let fields: Vec<&str> = line.split_whitespace().collect();

        if fields.len() < 14 {
            continue;
        }

        // Find the indices for the last few fixed fields by scanning from the end
        // TIME is always the last field
        // STARTED can be one or two parts (e.g., "12:46:12" or "Jul 27")
        // STAT is before STARTED
        // TT is before STAT
        // And so on...

        let len = fields.len();
        if len >= 15 {
            let time = fields[len - 1];
            // STARTED can be "Jul 27" (2 parts) or "12:46:12" (1 part)
            // Check if third-to-last looks like a month AND second-to-last looks like a day number
            let start = if fields[len - 3].len() == 3
                && fields[len - 3].chars().all(|c| c.is_alphabetic())
                && fields[len - 2].chars().all(|c| c.is_ascii_digit())
                && fields[len - 2].len() <= 2
            {
                // Two-part start: "Jul 27"
                format!("{} {}", fields[len - 3], fields[len - 2])
            } else {
                // One-part start: "12:46:12"
                fields[len - 2].to_string()
            };

            // Adjust indices based on whether start is 1 or 2 parts
            let start_offset = if start.contains(' ') { 3 } else { 2 };
            let stat = fields[len - 1 - start_offset];
            let tty = fields[len - 2 - start_offset];
            let rss = fields[len - 3 - start_offset];
            let vsz = fields[len - 4 - start_offset];
            let mem = fields[len - 5 - start_offset];
            let cpu = fields[len - 6 - start_offset];

            // The first 6 fields are always fixed: PID, PPID, USER, UID, GID, COMMAND
            let pid = fields[0];
            let ppid = fields[1];
            let user = fields[2];
            let uid = fields[3];
            let gid = fields[4];
            let comm = fields[5];

            // CMD is everything between field 6 and the fixed fields at the end
            let cmd_start = 6;
            let cmd_end = len - 6 - start_offset; // 6 fields: %CPU, %MEM, VSZ, RSS, TT, STAT
            let cmd = if cmd_end > cmd_start {
                fields[cmd_start..cmd_end].join(" ")
            } else {
                // If there's no separate CMD, use COMMAND
                comm.to_string()
            };

            let process = ProcessInfo {
                pid: pid.to_string(),
                ppid: ppid.to_string(),
                user: user.to_string(),
                uid: uid.to_string(),
                gid: gid.to_string(),
                comm: comm.to_string(),
                cmd,
                cpu: cpu.to_string(),
                mem: mem.to_string(),
                vsz: format!("{} KB", vsz),
                rss: format!("{} KB", rss),
                tty: tty.to_string(),
                stat: stat.to_string(),
                start: start.to_string(),
                time: time.to_string(),
            };
            processes.push(process);
        }
    }
    processes
}
