// Integration tests for HTTP If-Range validation.
//
// Verifies that the HTTP backend correctly detects when a resume
// request gets a 200 OK (full content) instead of 206 Partial Content.

#[cfg(test)]
mod tests {
    use raria_http::backend::HttpBackend;

    #[test]
    fn detect_200_on_range_request_as_resource_changed() {
        // 200 response to a range request = resource changed.
        assert!(
            HttpBackend::is_resource_changed(200, true),
            "200 on range request means resource changed"
        );

        // 206 response = range accepted.
        assert!(
            !HttpBackend::is_resource_changed(206, true),
            "206 on range request means range accepted"
        );

        // 200 on a non-range request is normal.
        assert!(
            !HttpBackend::is_resource_changed(200, false),
            "200 on non-range request is normal"
        );

        // 416 Range Not Satisfiable.
        assert!(
            !HttpBackend::is_resource_changed(416, true),
            "416 is not resource-changed, it's range error"
        );
    }
}
