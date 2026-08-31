use std::time::Duration;
use super::metrics::{format_bytes, format_rss};

#[derive(Default)]
pub struct BenchResultRow {
    pub bench_id: String,
    pub bench_name: String,
    pub vm_jit_avg: Duration,
    pub vm_interp_avg: Duration,
    pub bun_pure_avg: Duration,
    pub node_pure_avg: Duration,
    pub deno_pure_avg: Duration,
}

pub fn print_table_row(name: &str, avg_time: Duration, rss: Option<usize>, heap: Option<usize>) {
    let time_str = format!("{:?}", avg_time);
    let rss_str = format_rss(rss);
    let heap_str = match heap {
        Some(h) => format_bytes(h),
        None => "-".to_string(),
    };
    eprintln!(
        "│ {:<18} │ {:>10} │ {:>10} │ {:>10} │",
        name, time_str, rss_str, heap_str
    );
}

pub fn print_comparative_footer(
    vm_jit_avg: Duration,
    vm_interpreter_avg: Duration,
    bun_pure_avg: Duration,
    bun_cli_avg: Duration,
    node_pure_avg: Duration,
    node_cli_avg: Duration,
    deno_pure_avg: Duration,
    deno_cli_avg: Duration,
) {
    let mut has_footer = false;
    let mut print_footer = |icon: &str, text: &str| {
        if !has_footer {
            eprintln!("├────────────────────┴────────────┴────────────┴────────────┤");
            has_footer = true;
        }
        let text_len = text.chars().count();
        let padding_len = 52_usize.saturating_sub(text_len);
        let padding = " ".repeat(padding_len);
        eprintln!("│  {} {} {} │", icon, text, padding);
    };

    if vm_jit_avg.as_nanos() > 0 {
        let jit_speedup = vm_interpreter_avg.as_nanos() as f64 / vm_jit_avg.as_nanos() as f64;
        let jit_text = format!("JIT is {:.2}x FASTER than Interpreter", jit_speedup);
        print_footer("🚀", &jit_text);
    }

    let vm_avg = if vm_jit_avg.as_nanos() > 0 { vm_jit_avg } else { vm_interpreter_avg };
    let engine_name = if vm_jit_avg.as_nanos() > 0 { "JIT" } else { "Interpreter" };

    if bun_pure_avg.as_nanos() > 0 {
        if vm_avg < bun_pure_avg {
            let speedup = bun_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Bun (pure)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / bun_pure_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Bun (pure)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if bun_cli_avg.as_nanos() > 0 {
        if vm_avg < bun_cli_avg {
            let speedup = bun_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Bun (CLI)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / bun_cli_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Bun (CLI)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if node_pure_avg.as_nanos() > 0 {
        if vm_avg < node_pure_avg {
            let speedup = node_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Node (pure)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_pure_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Node (pure)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if node_cli_avg.as_nanos() > 0 {
        if vm_avg < node_cli_avg {
            let speedup = node_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Node (CLI)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / node_cli_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Node (CLI)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if deno_pure_avg.as_nanos() > 0 {
        if vm_avg < deno_pure_avg {
            let speedup = deno_pure_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Deno (pure)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / deno_pure_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Deno (pure)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if deno_cli_avg.as_nanos() > 0 {
        if vm_avg < deno_cli_avg {
            let speedup = deno_cli_avg.as_nanos() as f64 / vm_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x FASTER than Deno (CLI)", engine_name, speedup);
            print_footer("✅", &text);
        } else {
            let slowdown = vm_avg.as_nanos() as f64 / deno_cli_avg.as_nanos() as f64;
            let text = format!("{} is {:.2}x SLOWER than Deno (CLI)", engine_name, slowdown);
            print_footer("⚠️", &text);
        }
    }

    if has_footer {
        let footer_bottom = "─".repeat(59);
        eprintln!("└{}┘", footer_bottom);
    } else {
        eprintln!("└────────────────────┴────────────┴────────────┴────────────┘");
    }
    eprintln!();
}

pub fn print_consolidated_summary(results: &[BenchResultRow]) {
    eprintln!("╔═══════════════════════════════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║                                  SUMMARY COMPARISON MATRIX                                        ║");
    eprintln!("╠═══════════════════╦══════════════╦══════════════╦══════════════╦══════════════╦══════════════╦════╣");
    eprintln!("║ Benchmark         ║ Eronom JIT   ║ Eronom Interp║ Node.js Pure ║ Bun Pure     ║ Deno Pure    ║ JIT║");
    eprintln!("╠═══════════════════╬══════════════╬══════════════╬══════════════╬══════════════╬══════════════╬════╣");

    for r in results {
        let jit_str = if r.vm_jit_avg.as_nanos() > 0 {
            format!("{:?}", r.vm_jit_avg)
        } else {
            "-".to_string()
        };
        let interp_str = format!("{:?}", r.vm_interp_avg);
        let node_str = if r.node_pure_avg.as_nanos() > 0 { format!("{:?}", r.node_pure_avg) } else { "-".to_string() };
        let bun_str = if r.bun_pure_avg.as_nanos() > 0 { format!("{:?}", r.bun_pure_avg) } else { "-".to_string() };
        let deno_str = if r.deno_pure_avg.as_nanos() > 0 { format!("{:?}", r.deno_pure_avg) } else { "-".to_string() };
        
        let speedup_str = if r.vm_jit_avg.as_nanos() > 0 {
            let s = r.vm_interp_avg.as_nanos() as f64 / r.vm_jit_avg.as_nanos() as f64;
            format!("{:.1}x", s)
        } else {
            "-".to_string()
        };

        eprintln!(
            "║ {:<17} ║ {:>12} ║ {:>12} ║ {:>12} ║ {:>12} ║ {:>12} ║ {:>2} ║",
            r.bench_id, jit_str, interp_str, node_str, bun_str, deno_str, speedup_str
        );
    }
    eprintln!("╚═══════════════════╩══════════════╩══════════════╩══════════════╩══════════════╩══════════════╩════╝");
    eprintln!();
}
