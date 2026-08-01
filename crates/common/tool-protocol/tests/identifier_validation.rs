use std::str::FromStr;

use tool_protocol::{IdError, ToolCallId, ToolId};

#[test]
fn tool_id_accepts_bare_and_namespaced_names() {
    assert_eq!(ToolId::new("read_file").unwrap().as_str(), "read_file");
    assert_eq!(
        ToolId::from_str("Grow:read_file").unwrap().as_str(),
        "Grow:read_file"
    );
}

#[test]
fn tool_id_rejects_invalid_shapes() {
    assert_eq!(ToolId::new("").unwrap_err(), IdError::Empty);
    for value in ["foo:bar:baz", "foo bar", ":foo", "foo:", "foo/bar"] {
        assert!(matches!(
            ToolId::new(value),
            Err(IdError::InvalidFormat { .. })
        ));
    }
}

#[test]
fn tool_call_id_rejects_empty() {
    assert_eq!(ToolCallId::new("").unwrap_err(), IdError::Empty);
}

#[test]
fn tool_call_id_uuid_v7_helper_is_unique_and_valid() {
    let first = ToolCallId::new_v7();
    let second = ToolCallId::new_v7();
    assert_ne!(first, second);
    for id in [&first, &second] {
        let parsed = uuid::Uuid::parse_str(id.as_str()).unwrap();
        assert_eq!(parsed.get_version_num(), 7);
    }
}
