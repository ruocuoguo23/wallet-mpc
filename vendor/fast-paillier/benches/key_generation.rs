use criterion::{criterion_group, criterion_main, Criterion};
use fast_paillier::DecryptionKey;
use std::time::Duration;

fn benchmark_key_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("DecryptionKey Generation");

    // Default to 50 samples unless CLI overrides it (e.g., --sample-size 10)
    group.sample_size(50);

    // Allow long-running key generation by giving Criterion a 10-minute budget
    group.measurement_time(Duration::from_secs(300));

    // Benchmark generating a 1536-bit safe-prime key pair
    group.bench_function("generate_1536bit", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            DecryptionKey::generate(&mut rng).unwrap()
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_key_generation);
criterion_main!(benches);
