use criterion::{criterion_group, criterion_main, Criterion};
use leaky_cauldron::Leaky;

fn bench_leaky(b: &mut Criterion) {
    let leaky = Leaky::new_with_params(1_000.0, 1_000);
    b.bench_function("leaky", |b| {
        b.iter(|| {
            leaky.start_one().ok();
        })
    });
}

criterion_group!(benches, bench_leaky);
criterion_main!(benches);
