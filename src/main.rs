use clap::Parser;
use serde::Deserialize;
use std::process::Command;
use tabled::Tabled;
use tabled::settings::object::Rows;
use tabled::settings::{Color, Style};

#[derive(Parser)]
#[command(about = "mem watcher")]
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
    let pids = args.pid.clone();

    let mut processes = Processes {
        process_info: Vec::new(),
    };

    for pid in pids {
        let output = Command::new("ps")
            .arg("-o")
            .arg("pid,ppid,user,uid,gid,comm,cmd,%cpu,%mem,vsz,rss,tty,stat,start,time")
            .arg("-p")
            .arg(pid.to_string())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr)?;
            eprintln!("Command failed: {}", stderr);
            std::process::exit(1);
        }

        let stdout = String::from_utf8(output.stdout)?;
        let process_info = parse_ps_output(&stdout);
        if process_info.is_empty() {
            println!("No process found with PID {}", pid);
            return Ok(());
        }
        processes.process_info.push(process_info);
    }

    let flattened_processes: Vec<ProcessInfo> =
        processes.process_info.into_iter().flatten().collect();

    let mut table = tabled::Table::new(&flattened_processes);
    table.with(Style::rounded());
    table.modify(Rows::first(), Color::FG_GREEN);
    // table.modify(Rows::last(), Color::FG_BRIGHT_CYAN);
    println!("{}", table);
    Ok(())
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
                && fields[len - 2].len() <= 2 {
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
