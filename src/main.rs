use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Instant;

use hartwood::decode::decode;
use hartwood::disasm::disassemble;
use hartwood::elf::Elf;
use hartwood::{Config, Machine};

const USAGE: &str = "usage:
  hartwood run <prog.elf> [--trace] [--max-cycles N] [--cycles-per-tick N] [--ram-mb N] [--stats] [--no-stdin]
  hartwood disasm <prog.elf>";

fn parse_num(s: &str) -> Result<u64, String> {
    let r = if let Some(h) = s.strip_prefix("0x") {
        u64::from_str_radix(h, 16)
    } else {
        s.replace('_', "").parse()
    };
    r.map_err(|_| format!("invalid number: {s}"))
}

fn run(args: &[String]) -> Result<i32, String> {
    let mut path = None;
    let mut cfg = Config::default();
    let mut trace = false;
    let mut stats = false;
    let mut stdin = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--trace" => trace = true,
            "--stats" => stats = true,
            "--no-stdin" => stdin = false,
            "--max-cycles" => cfg.max_cycles = Some(parse_num(it.next().ok_or("--max-cycles needs a value")?)?),
            "--cycles-per-tick" => cfg.cycles_per_tick = parse_num(it.next().ok_or("--cycles-per-tick needs a value")?)?.max(1),
            "--ram-mb" => cfg.ram_size = (parse_num(it.next().ok_or("--ram-mb needs a value")?)? as usize) << 20,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s => path = Some(s.to_string()),
        }
    }
    let path = path.ok_or("missing ELF path")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut m = Machine::new(cfg, Box::new(std::io::stdout()));
    m.load_elf(&bytes).map_err(|e| format!("{path}: {e}"))?;
    if trace {
        m.trace = Some(Box::new(std::io::BufWriter::new(std::io::stderr())));
    }
    if stdin {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            let mut input = std::io::stdin();
            while let Ok(n) = input.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    if tx.send(b).is_err() {
                        return;
                    }
                }
            }
        });
        m.input = Some(rx);
    }
    let start = Instant::now();
    let exit = m.run();
    let secs = start.elapsed().as_secs_f64();
    let _ = std::io::stdout().flush();
    if let Some(t) = m.trace.as_mut() {
        let _ = t.flush();
    }
    if stats {
        eprintln!(
            "[hartwood] exit={exit:?} instret={} cycles={} mtime={} host_time={:.3}s MIPS={:.1} tlb_hits={} tlb_misses={}",
            m.cpu.instret,
            m.cpu.cycles,
            m.bus.clint.mtime,
            secs,
            m.cpu.instret as f64 / secs / 1e6,
            m.cpu.tlb.hits,
            m.cpu.tlb.misses
        );
    }
    Ok(exit.code())
}

fn disasm(args: &[String]) -> Result<i32, String> {
    let path = args.first().ok_or("missing ELF path")?;
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let elf = Elf::parse(&bytes).map_err(|e| e.to_string())?;
    let mut out = std::io::BufWriter::new(std::io::stdout());
    for seg in &elf.segments {
        let mut off = 0usize;
        let d = &seg.data;
        while off + 2 <= d.len() {
            let lo = u16::from_le_bytes([d[off], d[off + 1]]) as u32;
            let raw = if lo & 3 == 3 && off + 4 <= d.len() {
                lo | (u16::from_le_bytes([d[off + 2], d[off + 3]]) as u32) << 16
            } else {
                lo
            };
            let inst = decode(raw);
            let pc = seg.paddr + off as u64;
            let hex = if inst.len == 2 {
                format!("{:04x}    ", raw & 0xffff)
            } else {
                format!("{raw:08x}")
            };
            let _ = writeln!(out, "{pc:016x}: {hex}  {}", disassemble(&inst, pc));
            off += inst.len as usize;
        }
    }
    Ok(0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        Some("disasm") => disasm(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match res {
        Ok(code) => ExitCode::from(code.clamp(0, 255) as u8),
        Err(e) => {
            eprintln!("hartwood: {e}");
            ExitCode::from(2)
        }
    }
}
