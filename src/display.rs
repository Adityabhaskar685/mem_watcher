use tabled::settings::object::{Rows};
use tabled::settings::{Color, Style};

use crate::proc::ProcessInfo;

// Threshold for highlighting rows red in the terminal table. 
const CPU_WARM_THRESHOLD: f64 = 50.0;
const MEM_WARM_THRESHOLD: f64 = 20.0;

/// Render `processes` as a coloured terminal table followed by a summary line. 
/// 
/// - Header row is green
/// - Any row whose CPU% >= 50 or MEM % >= 20 is red.
/// - Thread rows (name prefixed with `|-`) are excluded from summary stats.
pub fn display_table(processes: &[ProcessInfo]) {
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
            if cpu_value >= CPU_WARM_THRESHOLD {
                table.modify(Rows::single(row_index), Color::FG_RED);
            }
        }

        if let Ok(mem_val) = process.mem_percent.parse::<f64>() {
            if mem_val >= MEM_WARM_THRESHOLD {
                table.modify(Rows::single(row_index), Color::FG_RED);
            }
        }
    }

    println!("{}", table);
    print_summary(processes);
}

fn print_summary(processes: &[ProcessInfo]) {
    // Exclude thread rows (prefixed with "|-") from summary stats
    let main_processes: Vec<&ProcessInfo> = processes
        .iter()
        .filter(|p| !p.name.starts_with("|-"))
        .collect();
    
    let total = main_processes.len();
    if total == 0 {
        return;
    }
    
    let avg_cpu: f64 = main_processes 
        .iter()
        .filter_map(|p| p.cpu_percent.parse::<f64>().ok())
        .sum::<f64>()
        / total as f64;
    let avg_mem: f64 = main_processes
        .iter()
        .filter_map(|p| p.mem_percent.parse::<f64>().ok())
        .sum::<f64>()
        / total as f64;

    let total_rss: u64 = main_processes
        .iter()
        .filter_map(|p| p.rss_kb.parse::<u64>().ok())
        .sum();
    let total_fds: u64 = main_processes
        .iter()
        .filter_map(|p| p.file_descriptors.parse::<u64>().ok())
        .sum();

    println!(
        "\n📈 Summary: {} proc | CPU: {:.1}% | MEM: {:.1}% | RSS: {} KB | FDs: {}",
        total, avg_cpu, avg_mem, total_rss, total_fds
    );
}