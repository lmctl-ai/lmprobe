#[test]
#[ignore = "reader source API: tracked as v1.1 follow-up"]
fn gitleaks_detect_reader_test_detect_reader() {
  // Port marker for detect/reader_test.go TestDetectReader.
  // lmprobe exposes filesystem/git scan sources and an in-memory full-content
  // Layer-1 API, but no public reader API matching gitleaks DetectReader.
}

#[test]
#[ignore = "streaming reader source API: tracked as v1.1 follow-up"]
fn gitleaks_detect_reader_test_stream_detect_reader() {
  // Port marker for detect/reader_test.go TestStreamDetectReader.
  // Streaming reader chunking, read-error propagation, and boundary handling
  // are not currently an lmprobe-secrets public source mode.
}
