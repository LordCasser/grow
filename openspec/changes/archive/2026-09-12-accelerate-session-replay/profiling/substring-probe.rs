use std::{hint::black_box, time::Instant};
fn main() {
    let line = format!(r#"{{"method":"session/update","params":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"text":"{}"}}}}}}}}"#, "x".repeat(4096));
    let needle = "subagent_finished";
    let finder = memchr::memmem::Finder::new(needle);
    for (label, mode) in [("str_contains", 0), ("memmem_find", 1), ("memmem_finder", 2)] {
        let t = Instant::now();
        for _ in 0..100000 {
            let haystack = black_box(line.as_str());
            black_box(match mode {
                0 => haystack.contains(black_box(needle)),
                1 => memchr::memmem::find(haystack.as_bytes(), black_box(needle.as_bytes())).is_some(),
                _ => finder.find(haystack.as_bytes()).is_some(),
            });
        }
        println!("{label}: {:.2}ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}
