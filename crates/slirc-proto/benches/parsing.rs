//! Benchmarks for IRC message parsing and serialization.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use slirc_proto::{prefix::Prefix, Message, MessageRef};

/// Simple PING message
const SIMPLE_MESSAGE: &str = "PING :irc.example.com";

/// Message with prefix
const PREFIX_MESSAGE: &str = ":nick!user@host PRIVMSG #channel :Hello, world!";

/// Message with IRCv3 tags
const TAGGED_MESSAGE: &str = "@time=2023-01-01T00:00:00.000Z;msgid=abc123;+example/tag=value :nick!user@host PRIVMSG #channel :Hello with tags!";

/// Complex message with escaped tags
const COMPLEX_TAGS: &str = "@time=2023-01-01T12:00:00Z;msgid=msg-12345;+draft/reply=parent-id;batch=batch001;account=username :nick!user@host.example.com PRIVMSG #long-channel-name :This is a longer message with more content to parse";

/// Numeric response
const NUMERIC_RESPONSE: &str =
    ":irc.server.net 001 nickname :Welcome to the IRC Network nickname!user@host";

fn benchmark_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Message Parsing");

    group.bench_function("simple_ping", |b| {
        b.iter(|| {
            let msg: Message = black_box(SIMPLE_MESSAGE).parse().unwrap();
            black_box(msg)
        })
    });

    group.bench_function("with_prefix", |b| {
        b.iter(|| {
            let msg: Message = black_box(PREFIX_MESSAGE).parse().unwrap();
            black_box(msg)
        })
    });

    group.bench_function("with_tags", |b| {
        b.iter(|| {
            let msg: Message = black_box(TAGGED_MESSAGE).parse().unwrap();
            black_box(msg)
        })
    });

    group.bench_function("complex_tags", |b| {
        b.iter(|| {
            let msg: Message = black_box(COMPLEX_TAGS).parse().unwrap();
            black_box(msg)
        })
    });

    group.bench_function("numeric_response", |b| {
        b.iter(|| {
            let msg: Message = black_box(NUMERIC_RESPONSE).parse().unwrap();
            black_box(msg)
        })
    });

    group.finish();
}

fn benchmark_zero_copy_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("Zero-Copy Parsing");

    group.bench_function("simple_ping", |b| {
        b.iter(|| {
            let msg = MessageRef::parse(black_box(SIMPLE_MESSAGE)).unwrap();
            black_box(msg)
        })
    });

    group.bench_function("with_prefix", |b| {
        b.iter(|| {
            let msg = MessageRef::parse(black_box(PREFIX_MESSAGE)).unwrap();
            black_box(msg)
        })
    });

    group.bench_function("with_tags", |b| {
        b.iter(|| {
            let msg = MessageRef::parse(black_box(TAGGED_MESSAGE)).unwrap();
            black_box(msg)
        })
    });

    group.bench_function("complex_tags", |b| {
        b.iter(|| {
            let msg = MessageRef::parse(black_box(COMPLEX_TAGS)).unwrap();
            black_box(msg)
        })
    });

    group.bench_function("numeric_response", |b| {
        b.iter(|| {
            let msg = MessageRef::parse(black_box(NUMERIC_RESPONSE)).unwrap();
            black_box(msg)
        })
    });

    group.finish();
}

fn benchmark_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("Message Serialization");

    // Pre-parse messages for serialization benchmarks
    let simple: Message = SIMPLE_MESSAGE.parse().unwrap();
    let with_prefix: Message = PREFIX_MESSAGE.parse().unwrap();
    let with_tags: Message = TAGGED_MESSAGE.parse().unwrap();
    let complex: Message = COMPLEX_TAGS.parse().unwrap();

    group.bench_function("simple_ping", |b| {
        b.iter(|| {
            let s = black_box(&simple).to_string();
            black_box(s)
        })
    });

    group.bench_function("with_prefix", |b| {
        b.iter(|| {
            let s = black_box(&with_prefix).to_string();
            black_box(s)
        })
    });

    group.bench_function("with_tags", |b| {
        b.iter(|| {
            let s = black_box(&with_tags).to_string();
            black_box(s)
        })
    });

    group.bench_function("complex_tags", |b| {
        b.iter(|| {
            let s = black_box(&complex).to_string();
            black_box(s)
        })
    });

    group.finish();
}

fn benchmark_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("Message Construction");

    group.bench_function("privmsg_simple", |b| {
        b.iter(|| {
            let msg = Message::privmsg(black_box("#channel"), black_box("Hello, world!"));
            black_box(msg)
        })
    });

    group.bench_function("privmsg_with_tags", |b| {
        b.iter(|| {
            let msg = Message::privmsg(black_box("#channel"), black_box("Hello!"))
                .with_tag("time", Some("2023-01-01T12:00:00Z"))
                .with_tag("msgid", Some("abc123"));
            black_box(msg)
        })
    });

    group.bench_function("privmsg_full", |b| {
        b.iter(|| {
            let msg = Message::privmsg(black_box("#channel"), black_box("Hello!"))
                .with_tag("time", Some("2023-01-01T12:00:00Z"))
                .with_tag("msgid", Some("abc123"))
                .with_prefix(Prefix::new_from_str("nick!user@host"));
            black_box(msg)
        })
    });

    group.finish();
}

fn benchmark_round_trip(c: &mut Criterion) {
    let mut group = c.benchmark_group("Round Trip");

    let messages = vec![
        ("simple", SIMPLE_MESSAGE),
        ("prefix", PREFIX_MESSAGE),
        ("tagged", TAGGED_MESSAGE),
        ("complex", COMPLEX_TAGS),
    ];

    for (name, msg_str) in messages {
        group.bench_with_input(
            BenchmarkId::new("parse_serialize", name),
            msg_str,
            |b, s| {
                b.iter(|| {
                    let msg: Message = black_box(s).parse().unwrap();
                    let serialized = msg.to_string();
                    black_box(serialized)
                })
            },
        );
    }

    group.finish();
}

fn benchmark_zero_copy_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("Zero-Copy Batch");

    // Simulate a batch of 100 messages
    let messages: Vec<String> = (0..100).map(|i| format!("PING :server{}\r\n", i)).collect();
    let batch: String = messages.concat();

    group.bench_function("parse_100_messages", |b| {
        b.iter(|| {
            let mut count = 0;
            for line in black_box(&batch).lines() {
                if let Ok(msg) = MessageRef::parse(line) {
                    count += 1;
                    black_box(msg);
                }
            }
            black_box(count)
        })
    });

    // Compare with owned parsing
    group.bench_function("parse_100_messages_owned", |b| {
        b.iter(|| {
            let mut count = 0;
            for line in black_box(&batch).lines() {
                if let Ok(msg) = line.parse::<Message>() {
                    count += 1;
                    black_box(msg);
                }
            }
            black_box(count)
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_parsing,
    benchmark_zero_copy_parsing,
    benchmark_serialization,
    benchmark_construction,
    benchmark_round_trip,
    benchmark_zero_copy_batch,
);

criterion_main!(benches);
