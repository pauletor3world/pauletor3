//! Memória do servidor conforme a RAM do PC.

/// Heap automático em MB a partir da RAM total em MB.
/// 8 GB → 3 GB; 16 GB → 5 GB; 32 GB+ → 6 GB. Nunca mais que metade da RAM.
pub fn auto_heap_mb(total_ram_mb: u64) -> u64 {
    let gb = total_ram_mb as f64 / 1024.0;
    let heap_gb: u64 = if gb < 6.0 {
        2
    } else if gb < 10.0 {
        3
    } else if gb < 14.0 {
        4
    } else if gb < 24.0 {
        5
    } else {
        6
    };
    (heap_gb * 1024).min((total_ram_mb / 2).max(1024))
}

pub fn total_ram_mb() -> u64 {
    let mut s = sysinfo::System::new();
    s.refresh_memory();
    s.total_memory() / (1024 * 1024)
}

/// Heap efetivo: o manual (se definido e sensato) ou o automático.
pub fn effective_heap_mb(manual: Option<u64>) -> u64 {
    let total = total_ram_mb();
    match manual {
        Some(m) if m >= 1024 && m <= total.saturating_sub(1024) => m,
        _ => auto_heap_mb(total),
    }
}

/// Argumentos da JVM: heap fixo (Xms = Xmx) e G1GC com ajustes conservadores.
pub fn jvm_args(heap_mb: u64) -> Vec<String> {
    vec![
        format!("-Xms{heap_mb}M"),
        format!("-Xmx{heap_mb}M"),
        "-XX:+UseG1GC".into(),
        "-XX:+ParallelRefProcEnabled".into(),
        "-XX:MaxGCPauseMillis=200".into(),
        "-XX:+DisableExplicitGC".into(),
        "-XX:+AlwaysPreTouch".into(),
        "-Dfile.encoding=UTF-8".into(),
        "-Dstdout.encoding=UTF-8".into(),
        "-Dstderr.encoding=UTF-8".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_table() {
        assert_eq!(auto_heap_mb(16 * 1024), 5 * 1024);
        assert_eq!(auto_heap_mb(8 * 1024), 3 * 1024);
        assert_eq!(auto_heap_mb(32 * 1024), 6 * 1024);
        assert_eq!(auto_heap_mb(4 * 1024), 2 * 1024);
        assert_eq!(auto_heap_mb(3 * 1024), 1536, "nunca mais que metade da RAM");
        assert!(jvm_args(3072).contains(&"-XX:+UseG1GC".to_string()));
    }
}
