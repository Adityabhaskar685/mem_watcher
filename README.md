# mem_watcher
 
A lightweight Linux process monitor that reads directly from the `/proc` filesystem — no external tools like `ps`, `top`, or `htop` required.
 
Monitor memory usage, CPU%, file descriptors, uptime, and thread details for one or more processes, either as a single snapshot or as a live continuous feed.
 
---
 
## Features
 
- Reads `/proc/{pid}/stat`, `/proc/{pid}/statm`, and `/proc/{pid}/fd` directly
- Accurate CPU% calculation using system-wide CPU time as the baseline
- Real RSS and VSZ memory in KB with percentage of total system memory
- File descriptor count per process
- Process uptime and accumulated CPU time
- Optional thread-level breakdown with `--show-threads`
- Continuous monitoring mode with configurable interval and duration
- Color-coded output: green headers, red rows for high CPU (≥50%) or high memory (≥10%)
- Summary line with averages across all monitored processes
 
---
 
## Requirements
 
- Linux (uses `/proc` filesystem — not available on macOS or Windows)
- Rust 1.85+ (edition 2024)
 
---
 
## Installation
 
```bash
git clone https://github.com/Adityabhaskar685/mem_watcher
cd mem_watcher
cargo build --release
```
 
The binary will be at `target/release/mem_watcher`.
 
Optionally install it system-wide:
 
```bash
cargo install --path .
```
 
---
 
## Usage
 
```
mem_watcher -p <PID> [OPTIONS]
```
 
### Options
 
| Flag | Short | Default | Description |
|------|-------|---------|-------------|
| `--process_id` | `-p` | required | One or more PIDs to monitor |
| `--duration` | `-d` | — | Monitor for N seconds (omit for single snapshot) |
| `--interval` | `-i` | `2` | Refresh interval in seconds (continuous mode) |
| `--show-threads` | — | off | Show individual threads under each process |
| `--no-clear` | — | off | Don't clear the screen between updates |
 
---
 
## Examples
 
**Single snapshot of one process:**
```bash
mem_watcher -p 1234
```
 
**Monitor multiple processes for 60 seconds, refreshing every 5s:**
```bash
mem_watcher -p 1234 -p 5678 -d 60 -i 5
```
 
**Inspect a process with spaces in its name (e.g. `tmux: server`) and its threads:**
```bash
mem_watcher -p 6733 --show-threads
```
 
**Continuous monitoring without clearing the screen (useful for piping/logging):**
```bash
mem_watcher -p 1234 -d 120 --no-clear
```
 
---
 
## Output
 
```
╭───────┬──────┬──────────────┬───────┬──────┬──────┬────────┬────────┬─────────┬─────┬──────────┬──────────╮
│ PID   │ PPID │ NAME         │ STATE │ %CPU │ %MEM │ RSS KB │ VSZ KB │ THREADS │ FDS │ CPU_TIME │ UPTIME   │
├───────┼──────┼──────────────┼───────┼──────┼──────┼────────┼────────┼─────────┼─────┼──────────┼──────────┤
│ 6733  │ 1    │ tmux: server │ S     │ 0.0  │ 0.1  │ 3412   │ 10240  │ 1       │ 12  │ 4m32s    │ 2h15m8s  │
│ 6734  │ 6733 │ |- bash      │ S     │ 0.0  │ 0.0  │ 1820   │ 8192   │ 1       │ 5   │ 0m3s     │ 2h15m7s  │
╰───────┴──────┴──────────────┴───────┴──────┴──────┴────────┴────────┴─────────┴─────┴──────────┴──────────╯
 
📈 Summary: 1 proc | CPU: 0.0% | MEM: 0.1% | RSS: 3412 KB | FDs: 12
```
 
### State codes
 
| Code | Meaning |
|------|---------|
| `R` | Running |
| `S` | Sleeping (interruptible) |
| `D` | Waiting (uninterruptible disk sleep) |
| `Z` | Zombie |
| `T` | Stopped or traced |
| `I` | Idle kernel thread |
 
---
 
## How CPU% is calculated
 
CPU usage is computed as:
 
```
%CPU = (process_utime_delta / system_cpu_time_delta) × 100 × cpu_count
```
 
- `process_utime_delta` — change in the process's `utime + stime` ticks between two samples
- `system_cpu_time_delta` — change in total system CPU ticks across all cores (`/proc/stat`)
- Child process times (`cutime`, `cstime`) are deliberately excluded to avoid inflating the percentage for processes that reap children
 
On the first snapshot, a 200ms warm-up sample is taken automatically so the reading is never stuck at 0%.
 
---
 
## Dependencies
 
| Crate | Purpose |
|-------|---------|
| [`clap`](https://crates.io/crates/clap) | Argument parsing |
| [`tabled`](https://crates.io/crates/tabled) | Terminal table rendering |
| [`chrono`](https://crates.io/crates/chrono) | Timestamp formatting in continuous mode |
| [`sysconf`](https://crates.io/crates/sysconf) | Reading `SC_CLK_TCK` and `SC_PAGESIZE` at startup |
 
---
 
## Limitations
 
- Linux only — depends on the `/proc` filesystem
- Requires read access to `/proc/{pid}/fd` for file descriptor counts; this may return 0 for processes owned by other users unless run as root
- CPU% for the very first interval in continuous mode reflects only the warm-up window (~200ms), not the full interval
 
---
