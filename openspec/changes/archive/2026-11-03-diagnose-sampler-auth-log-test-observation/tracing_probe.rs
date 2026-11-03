use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

#[derive(Default)]
struct Capture(Mutex<Vec<u8>>);

impl Capture {
    fn push(&self, line: impl AsRef<str>) {
        let mut bytes = self.0.lock().unwrap();
        bytes.extend_from_slice(line.as_ref().as_bytes());
        bytes.push(b'\n');
    }
}

struct Recorder(Arc<Capture>);

impl Subscriber for Recorder {
    fn enabled(&self, _: &Metadata<'_>) -> bool { true }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        self.0.push(format!("span:{}", attrs.metadata().name()));
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = Fields::default();
        event.record(&mut visitor);
        self.0.push(format!("event:{}:{}", event.metadata().name(), visitor.0));
    }

    fn enter(&self, _: &Id) { self.0.push("enter"); }
    fn exit(&self, _: &Id) { self.0.push("exit"); }

    fn register_callsite(&self, _: &'static Metadata<'static>) -> tracing::subscriber::Interest {
        tracing::subscriber::Interest::sometimes()
    }

    fn max_level_hint(&self) -> Option<tracing::metadata::LevelFilter> {
        Some(tracing::metadata::LevelFilter::INFO)
    }
}

#[derive(Default)]
struct Fields(String);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "probe_id" { self.0 = format!("{value:?}"); }
    }
}

// Both macros are at one source location, so all threads exercise the same cached callsites.
fn fire(probe_id: usize) {
    let span = tracing::info_span!("cached_probe_span", probe_id);
    let _entered = span.enter();
    tracing::info!(probe_id, "cached_probe_event");
}

fn main() {
    const WORKERS: usize = 4;
    const CYCLES_PER_WORKER: usize = 25_000;
    let start = Arc::new(Barrier::new(WORKERS));
    let mut handles = Vec::new();
    let captures: Vec<_> = (0..WORKERS).map(|_| Arc::new(Capture::default())).collect();

    for (worker, capture) in captures.iter().cloned().enumerate() {
        let start = start.clone();
        handles.push(thread::spawn(move || {
            let dispatch = tracing::Dispatch::new(Recorder(capture));
            let no_subscriber = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::new());
            start.wait();
            for i in 0..CYCLES_PER_WORKER {
                let probe_id = worker * CYCLES_PER_WORKER + i;
                tracing::dispatcher::with_default(&dispatch, || fire(probe_id));
                tracing::dispatcher::with_default(&no_subscriber, || fire(probe_id));
            }
        }));
    }

    for handle in handles { handle.join().unwrap(); }
    let expected_events = WORKERS * CYCLES_PER_WORKER;
    let mut total_events = 0;
    let mut total_spans = 0;
    let mut total_enters = 0;
    let mut total_exits = 0;
    for (worker, capture) in captures.iter().enumerate() {
        let bytes = capture.0.lock().unwrap().clone();
        let records = String::from_utf8(bytes).unwrap();
        let events = records.lines().filter(|line| line.starts_with("event:")).count();
        let spans = records.lines().filter(|line| line.starts_with("span:")).count();
        let enters = records.lines().filter(|line| *line == "enter").count();
        let exits = records.lines().filter(|line| *line == "exit").count();
        println!("worker={worker} events={events} spans={spans} enters={enters} exits={exits}");
        total_events += events;
        total_spans += spans;
        total_enters += enters;
        total_exits += exits;
    }
    println!("expected_on_dispatcher={expected_events} events={total_events} spans={total_spans} enters={total_enters} exits={total_exits}");
    if total_events != expected_events || total_spans != expected_events || total_enters != expected_events || total_exits != expected_events {
        eprintln!("LOSS detected");
        std::process::exit(2);
    }
    println!("NO LOSS in probe");
}
