use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use slirc_proto::{Message, Command, Prefix};

// Simple benchmark to measure message parsing and routing overhead
// Note: This is an integration benchmark, so it might need some mocking if we want to isolate components.
// For now, we'll benchmark pure message creation and cloning as a baseline.

fn message_creation_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("message");
    group.throughput(Throughput::Elements(1));

    group.bench_function("create_privmsg", |b| {
        b.iter(|| {
            Message {
                tags: None,
                prefix: Some(Prefix::Nickname("sender".to_string(), "user".to_string(), "host".to_string())),
                command: Command::PRIVMSG("#channel".to_string(), "Hello world".to_string()),
            }
        })
    });
    
    group.finish();
}

fn message_parsing_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing");
    let raw = b"@time=2023-01-01T12:00:00.000Z :sender!user@host PRIVMSG #channel :Hello world\r\n";
    group.throughput(Throughput::Bytes(raw.len() as u64));

    group.bench_function("parse_privmsg", |b| {
        b.iter(|| {
            std::str::from_utf8(raw).unwrap().parse::<Message>().unwrap()
        })
    });

    group.finish();
}

criterion_group!(benches, message_creation_benchmark, message_parsing_benchmark);
criterion_main!(benches);
