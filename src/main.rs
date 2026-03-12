mod display;
mod proc;

use clap::Parser;
use std::thread;
use std::time::{Duration, Instant};

use display::display_table;
use proc::{CpuStats, get_process_data_realtime, prime_cpu_baseline};

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

    // Warm-up: take an initial CPU sample, sleep briefly, then sample again.
    // Without this the first snapshot always show 0% CPU becausee there is no
    // previous measurement to diff against.
    let mut cpu_stats = CpuStats::new()?;
    prime_cpu_baseline(&args.pid, &mut cpu_stats)?;
    thread::sleep(Duration::from_millis(200));

    let processes = get_process_data_realtime(&args.pid, &mut cpu_stats, args.show_threads)?;
    display_table(&processes);

    Ok(())
}
