use criterion::{criterion_group, criterion_main, Criterion};
use key_gen::dealer::{KeyGenConfig, KeyShareDealer};
use rand::RngCore;
use rand::rngs::StdRng;
use rand::SeedableRng;

fn random_child_key(rng: &mut impl RngCore) -> [u8; 32] {
    let mut key = [0u8; 32];
    rng.fill_bytes(&mut key);
    key
}

fn build_config(rng: &mut impl RngCore) -> KeyGenConfig {
    KeyGenConfig {
        n_parties: 2,
        threshold: 2,
        account_id: "bench-account".to_owned(),
        child_key: random_child_key(rng),
        output_prefix: "bench_output".to_owned(),
        pubkeys: None,
    }
}

fn benchmark_generate_shares(c: &mut Criterion) {
    let mut group = c.benchmark_group("dealer_generate_shares");
    // Default to 10 samples so Criterion's minimum requirement is satisfied.
    group.sample_size(30);

    let mut rng = StdRng::seed_from_u64(42);

    group.bench_function("generate_shares_2of2", |b| {
        b.iter(|| {
            let config = build_config(&mut rng);
            let mut dealer = KeyShareDealer::new(config).expect("config is valid");
            dealer.generate_shares().expect("generation succeeds");
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_generate_shares);
criterion_main!(benches);
