mod display;
mod proc;
mod tui;
 
use clap::Parser;
use std::thread;
use std::time::{Duration, Instant};
 
use display::display_table;
use proc::{CpuStats, get_process_data_realtime, prime_cpu_baseline};
 
#[derive(Parser)]
#[command(name = "mem_watcher")]
#[command(about = "Process watcher using /proc — TUI or CLI")]
#[command(version = "1.0")]
struct Args {
    /// One or more PIDs to monitor (CLI mode).
    /// Not required when using --tui.
    #[arg(short = 'p', long = "process_id", help = "One or more PIDs (CLI mode)")]
    pid: Vec<i32>,
 
    /// Launch the interactive TUI (process selector + live monitor).
    #[arg(long = "tui", help = "Launch interactive TUI")]
    tui: bool,
 
    /// Monitor duration in seconds (CLI continuous mode; omit for snapshot).
    #[arg(short = 'd', long = "duration", help = "Monitor duration in seconds")]
    duration: Option<u64>,
 
    /// Refresh interval in seconds (CLI continuous mode).
    #[arg(short = 'i', long = "interval", default_value = "2")]
    interval: u64,
 
    /// Don't clear the screen between updates (CLI continuous mode).
    #[arg(long = "no-clear")]
    no_clear: bool,
 
    /// Show individual threads under each process.
    #[arg(long = "show-threads")]
    show_threads: bool,
}
 
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
 
    if args.tui {
        // TUI mode — ignores -p / -d / -i / --no-clear
        return tui::run_tui(args.show_threads);
    }
 
    if args.pid.is_empty() {
        eprintln!("error: provide --tui for interactive mode, or -p <PID> for CLI mode.");
        std::process::exit(1);
    }
 
    match args.duration {
        Some(duration) => monitor_continuously(&args, duration)?,
        None => display_snapshot(&args)?,
    }
 
    Ok(())
}
 
fn display_snapshot(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    println!("Process snapshot for PIDs {:?}", args.pid);
    println!("{}", "=".repeat(80));
 
    let mut cpu_stats = CpuStats::new()?;
    prime_cpu_baseline(&args.pid, &mut cpu_stats)?;
    thread::sleep(Duration::from_millis(200));
 
    let processes = get_process_data_realtime(&args.pid, &mut cpu_stats, args.show_threads)?;
    display_table(&processes);
 
    Ok(())
}
 
fn monitor_continuously(
    args: &Args,
    duration_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔍 Monitoring {:?} for {}s (interval: {}s)",
        args.pid, duration_secs, args.interval
    );
    println!("Press Ctrl+C to stop early");
    println!("{}", "=".repeat(80));
 
    let start = Instant::now();
    let duration = Duration::from_secs(duration_secs);
    let interval = Duration::from_secs(args.interval);
    let mut cpu_stats = CpuStats::new()?;
 
    while start.elapsed() < duration {
        let tick_start = Instant::now();
 
        if !args.no_clear {
            print!("\x1B[2J\x1B[1;1H");
        }
 
        let elapsed = start.elapsed().as_secs();
        println!(
            "⏱️  {}s elapsed | {}s remaining | {}",
            elapsed,
            duration_secs.saturating_sub(elapsed),
            chrono::Local::now().format("%H:%M:%S")
        );
        println!("{}", "-".repeat(80));
 
        match get_process_data_realtime(&args.pid, &mut cpu_stats, args.show_threads) {
            Ok(processes) => display_table(&processes),
            Err(e) => eprintln!("❌ Error: {}", e),
        }
 
        let spent = tick_start.elapsed();
        if spent < interval {
            thread::sleep(interval - spent);
        }
    }
 
    println!("\n✅ Monitoring completed after {} seconds", duration_secs);
    Ok(())
}