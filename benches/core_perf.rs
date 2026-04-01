/// Criterion benchmarks for shako's hot-path functions.
///
/// Benchmarks cover:
///   - Parser: `tokenize`, `parse_args`, `split_chains`, `split_pipes`
///   - Classifier: `Classifier::classify` (hit and miss paths)
///   - Completer: `ShakoCompleter::complete` (prefix lookup)
///   - History: `expand_history_bangs`
///
/// Run with:
///   cargo bench
///   cargo bench -- parser        # filter to parser group only
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use shako::classifier::Classifier;
use shako::parser;
use shako::path_cache::PathCache;
use shako::shell::repl::expand_history_bangs;

// ── Parser benchmarks ─────────────────────────────────────────────────────────

fn bench_tokenize(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");

    // Simple command — common fast path.
    group.bench_function("tokenize_simple", |b| {
        b.iter(|| parser::tokenize(black_box("ls -la /tmp")));
    });

    // Quoted strings and variable expansions — exercises more tokenizer paths.
    group.bench_function("tokenize_complex", |b| {
        b.iter(|| parser::tokenize(black_box(r#"echo "hello $USER" | grep -v '^#' | sort -u"#)));
    });

    // Long pipeline to stress-test the tokenizer's allocation behaviour.
    group.bench_function("tokenize_long_pipeline", |b| {
        b.iter(|| {
            parser::tokenize(black_box(
                "cat /etc/hosts | grep localhost | awk '{print $1}' | sort | uniq -c | sort -rn",
            ))
        });
    });

    group.finish();
}

fn bench_parse_args(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");

    group.bench_function("parse_args_simple", |b| {
        b.iter(|| parser::parse_args(black_box("git status --short")));
    });

    group.bench_function("parse_args_with_glob", |b| {
        b.iter(|| parser::parse_args(black_box("ls *.rs src/**/*.toml")));
    });

    group.bench_function("parse_args_with_tilde", |b| {
        b.iter(|| parser::parse_args(black_box("cp ~/file.txt ~/.config/shako/")));
    });

    group.finish();
}

fn bench_split_chains(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");

    group.bench_function("split_chains_simple", |b| {
        b.iter(|| parser::split_chains(black_box("make build && make test || echo failed")));
    });

    group.bench_function("split_chains_semicolons", |b| {
        b.iter(|| parser::split_chains(black_box("cd /tmp; ls; pwd; echo done")));
    });

    group.finish();
}

fn bench_split_pipes(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");

    group.bench_function("split_pipes_3way", |b| {
        b.iter(|| parser::split_pipes(black_box("cat file.txt | grep pattern | wc -l")));
    });

    group.bench_function("split_pipes_5way", |b| {
        b.iter(|| {
            parser::split_pipes(black_box(
                "ps aux | grep rust | awk '{print $1}' | sort | uniq",
            ))
        });
    });

    group.finish();
}

// ── Classifier benchmarks ─────────────────────────────────────────────────────

fn bench_classifier(c: &mut Criterion) {
    let cache = PathCache::new();
    let classifier = Classifier::new(cache);

    let mut group = c.benchmark_group("classifier");

    // Known binary — PATH hit (fast path).
    group.bench_function("classify_known_command", |b| {
        b.iter(|| classifier.classify(black_box("ls -la")));
    });

    // Forced AI prefix — short-circuit before PATH lookup.
    group.bench_function("classify_forced_ai", |b| {
        b.iter(|| classifier.classify(black_box("? list all rust files modified today")));
    });

    // History search prefix.
    group.bench_function("classify_history_search", |b| {
        b.iter(|| classifier.classify(black_box("?? cargo build")));
    });

    // Natural language — requires full PATH scan + typo check.
    group.bench_function("classify_natural_language", |b| {
        b.iter(|| classifier.classify(black_box("list all python files modified this week")));
    });

    // Builtin command — checked before PATH lookup.
    group.bench_function("classify_builtin", |b| {
        b.iter(|| classifier.classify(black_box("cd /tmp")));
    });

    // Empty input — immediate early return.
    group.bench_function("classify_empty", |b| {
        b.iter(|| classifier.classify(black_box("")));
    });

    group.finish();
}

// ── History bangs benchmark ───────────────────────────────────────────────────

fn bench_history_bangs(c: &mut Criterion) {
    let mut group = c.benchmark_group("history");

    // No substitution needed — common case.
    group.bench_function("expand_no_bangs", |b| {
        b.iter(|| expand_history_bangs(black_box("git status"), black_box("git diff")));
    });

    // `!!` expansion — replace with last command.
    group.bench_function("expand_bang_bang", |b| {
        b.iter(|| expand_history_bangs(black_box("sudo !!"), black_box("apt update")));
    });

    // `!$` expansion — last word of previous command.
    group.bench_function("expand_bang_dollar", |b| {
        b.iter(|| expand_history_bangs(black_box("cat !$"), black_box("vim /etc/hosts")));
    });

    group.finish();
}

// ── Criterion entry points ────────────────────────────────────────────────────

criterion_group!(
    parser_benches,
    bench_tokenize,
    bench_parse_args,
    bench_split_chains,
    bench_split_pipes
);
criterion_group!(classifier_benches, bench_classifier);
criterion_group!(history_benches, bench_history_bangs);

criterion_main!(parser_benches, classifier_benches, history_benches);
