
    // BEGIN BACKLOG SEARCH COST PROBE
    #[test]
    #[ignore = "temporary diagnostic"]
    fn backlog_search_cost_probe() {
        use std::io::Write as _;
        use std::time::Instant;
        for turns in [128, 512, 2048] {
            let mut timeline = Timeline::default();
            for i in 0..turns {
                record_search_turn(&mut timeline, i as u64 + 1, i, &format!("prompt {i} café"), &"answer ".repeat(128), vec![]);
            }
            let bytes = encode_timeline(&timeline);
            let expected = timeline_indexable_content(&timeline);
            let mut file = tempfile::NamedTempFile::new().unwrap();
            file.write_all(&bytes).unwrap(); file.flush().unwrap();
            let mut samples = Vec::new();
            for run in 0..9 {
                let reader = TimelineLedgerReader::from_file(file.reopen().unwrap(), file.path().to_owned()).unwrap();
                let start = Instant::now();
                let events = reader.read_events().unwrap();
                let decode = start.elapsed();
                let start = Instant::now();
                let rebuilt = Timeline::from_events(events).unwrap();
                let fold = start.elapsed();
                let start = Instant::now();
                let content = timeline_indexable_content(&rebuilt);
                let extract = start.elapsed();
                assert_eq!(content, expected);
                if run >= 2 { samples.push([decode.as_nanos(), fold.as_nanos(), extract.as_nanos()]); }
            }
            println!("SEARCH_COST turns={turns} bytes={} output={} samples_ns={samples:?}", bytes.len(), expected.len());
        }
    }
    // END BACKLOG SEARCH COST PROBE
