pub mod client;
pub mod server;

/// Do not echo untrusted request values: malformed requests may contain private text too.
pub(crate) fn decode_request(bytes: &[u8]) -> Result<niri_ipc::Request, String> {
    serde_json::from_slice(bytes).map_err(|err| {
        format!(
            "invalid IPC request ({:?}) at line {}, column {}",
            err.classify(),
            err.line(),
            err.column()
        )
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn malformed_private_requests_do_not_echo_values() {
        let secret = "private transcript that must never be logged";
        for value in [
            serde_json::json!({"PasteText": secret}),
            serde_json::json!({"PasteText": {"id": secret, "text": secret}}),
            serde_json::json!({"PasteText": {"id": 12, "text": secret, "shortcut": secret}}),
            serde_json::json!({"CommitText": {"id": 12, "text": secret, "context_revision": secret}}),
        ] {
            let error = super::decode_request(&serde_json::to_vec(&value).unwrap()).unwrap_err();
            assert!(!error.contains(secret));
            assert!(error.starts_with("invalid IPC request (Data) at line "));
        }
        assert!(matches!(
            super::decode_request(b"\"Version\""),
            Ok(niri_ipc::Request::Version)
        ));
    }
}
