use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures").join(name)
}

#[test]
fn decodes_opus_to_16k_mono() {
    let samples = vscribe_lib::engine::decode(&fixture("jfk.opus")).unwrap();
    let seconds = samples.len() as f32 / vscribe_lib::engine::SAMPLE_RATE as f32;
    assert!((seconds - 11.0).abs() < 0.1, "decoded {seconds}s");
    assert!(samples.iter().any(|s| s.abs() > 0.1));
}

#[test]
fn rejects_files_that_are_not_audio() {
    let dir = std::env::temp_dir().join("vscribe-decode-test");
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.ogg");
    std::fs::write(&junk, b"not audio at all").unwrap();
    assert!(matches!(vscribe_lib::engine::decode(&junk), Err(vscribe_lib::engine::Error::Undecodable(_))));
}
