use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use messloc::blind::size_class::*;

pub fn criterion_benchmark(c: &mut Criterion) {
    let size = 2_usize.pow(14);
    c.bench_with_input(BenchmarkId::new("size class many", size), &size, |b, &s| {
        b.iter(|| find_size_class_index(s));
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
