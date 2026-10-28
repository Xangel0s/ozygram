use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ozymem_core::engram_store::reader::FastEngramReader;
use ozymem_parser::{EngramContract, EngramTable};
use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

fn setup_benchmark_engram_file(num_symbols: usize) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("bench_engrams.rkyv");

    let mut table = EngramTable::new();
    for i in 0..num_symbols {
        let contract = EngramContract {
            symbol_path: format!("src/module_{}/service_{}.rs::process_item_{}", i % 50, i % 20, i),
            file_path: format!("src/module_{}/service_{}.rs", i % 50, i % 20),
            name: format!("process_item_{}", i),
            signature: format!("pub async fn process_item_{}(ctx: &Context, id: u64) -> Result<Item>", i),
            line_number: (i * 10) as u32,
            end_line: (i * 10 + 25) as u32,
            kind: "function".to_string(),
            language: "rust".to_string(),
            dependencies: vec!["Context".to_string(), "Result".to_string(), "Item".to_string()],
            doc_summary: "Processes an incoming data payload with zero-copy validation.".to_string(),
        };
        table.insert(contract);
    }

    let bytes = rkyv::to_bytes::<_, 1024>(&table).expect("serialize engram table");
    let mut file = File::create(&path).expect("create bench file");
    file.write_all(&bytes).expect("write bytes");
    file.flush().expect("flush bytes");

    (dir, path)
}

fn bench_engram_lookups(c: &mut Criterion) {
    let (_dir, path) = setup_benchmark_engram_file(5000);
    let reader = FastEngramReader::open(&path).expect("open fast reader");
    let query_symbol = "src/module_25/service_10.rs::process_item_2510";

    let mut group = c.benchmark_group("engram_lookup_5000_symbols");

    // 1. Zero-Copy real: directo sobre buffer mmap sin allocar
    group.bench_function("lookup_archived_zero_copy", |b| {
        b.iter(|| {
            let res = reader.lookup_archived(black_box(query_symbol));
            black_box(res);
        })
    });

    // 2. Owned tradicional: deserialización con ~10 Strings en heap
    group.bench_function("lookup_owned_deserialized", |b| {
        b.iter(|| {
            let res = reader.lookup(black_box(query_symbol));
            black_box(res);
        })
    });

    group.finish();
}

criterion_group!(benches, bench_engram_lookups);
criterion_main!(benches);
