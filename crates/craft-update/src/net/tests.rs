use super::*;

#[test]
fn metadata_body_accepts_its_exact_limit() {
    let expected = b"[{\"tag_name\":\"v0.5.0\"}]";
    let mut body = ureq::Body::builder().data(expected.to_vec());
    assert_eq!(read_body(&mut body, expected.len() as u64).unwrap(), expected);
}

#[test]
fn metadata_body_accepts_an_empty_body_with_a_zero_limit() {
    let mut body = ureq::Body::builder().data(Vec::new());
    assert!(read_body(&mut body, 0).unwrap().is_empty());
}

#[test]
fn metadata_body_refuses_bytes_beyond_its_limit() {
    let mut body = ureq::Body::builder().data(b"12345".to_vec());
    assert!(read_body(&mut body, 4).is_err());
    let mut body = ureq::Body::builder().data(b"1".to_vec());
    assert!(read_body(&mut body, 0).is_err());
}

#[test]
fn metadata_body_handles_the_largest_limit_without_overflow() {
    let mut body = ureq::Body::builder().data(b"[]".to_vec());
    assert_eq!(read_body(&mut body, u64::MAX).unwrap(), b"[]");
}

#[test]
fn streamed_body_accepts_its_exact_limit() {
    let expected = b"signed package";
    let mut body = ureq::Body::builder().data(expected.to_vec());
    let mut received = Vec::new();
    stream_body(&mut body, expected.len() as u64, &mut |chunk| {
        received.extend_from_slice(chunk);
        Ok(())
    })
    .unwrap();
    assert_eq!(received, expected);
}

#[test]
fn streamed_body_accepts_an_exact_64_kib_boundary() {
    let expected = vec![73; 64 * 1024];
    let mut body = ureq::Body::builder().data(expected.clone());
    let mut received = Vec::new();
    stream_body(&mut body, expected.len() as u64, &mut |chunk| {
        received.extend_from_slice(chunk);
        Ok(())
    })
    .unwrap();
    assert_eq!(received, expected);
}

#[test]
fn streamed_body_delivers_multiple_chunks_and_the_final_partial_chunk() {
    let expected: Vec<u8> = (0..(2 * 64 * 1024 + 37)).map(|index| (index % 251) as u8).collect();
    let mut body = ureq::Body::builder().data(expected.clone());
    let mut received = Vec::new();
    let mut chunks = 0;
    stream_body(&mut body, expected.len() as u64, &mut |chunk| {
        assert!(!chunk.is_empty());
        chunks += 1;
        received.extend_from_slice(chunk);
        Ok(())
    })
    .unwrap();
    assert_eq!(received, expected);
    assert!(chunks > 1);
}

#[test]
fn streamed_body_accepts_an_empty_body_with_a_zero_limit() {
    let mut body = ureq::Body::builder().data(Vec::new());
    let mut calls = 0;
    stream_body(&mut body, 0, &mut |_| {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 0);
}

#[test]
fn streamed_body_refuses_any_bytes_with_a_zero_limit() {
    let mut body = ureq::Body::builder().data(b"1".to_vec());
    let mut calls = 0;
    let result = stream_body(&mut body, 0, &mut |_| {
        calls += 1;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(calls, 0);
}

#[test]
fn streamed_body_never_delivers_bytes_beyond_the_limit() {
    let limit = 64 * 1024;
    let payload = vec![42; limit + 1];
    let mut body = ureq::Body::builder().data(payload.clone());
    let mut received = Vec::new();
    let result = stream_body(&mut body, limit as u64, &mut |chunk| {
        received.extend_from_slice(chunk);
        Ok(())
    });
    assert!(result.is_err());
    assert!(received.len() <= limit);
    assert_eq!(received, payload[..received.len()]);
}

#[test]
fn streamed_body_preserves_sink_cancellation_and_stops_delivering() {
    let payload = vec![42; 2 * 64 * 1024 + 1];
    let mut body = ureq::Body::builder().data(payload.clone());
    let mut calls = 0;
    let result = stream_body(&mut body, payload.len() as u64, &mut |_| {
        calls += 1;
        Err(Error::Cancelled)
    });
    assert_eq!(result, Err(Error::Cancelled));
    assert_eq!(calls, 1);
}

/// Live: asks GitHub over TLS with the OS's roots (`cargo test -p craft-update -- --ignored`).
#[test]
#[ignore = "needs network access"]
fn github_answers_with_a_release_list() {
    let body = GitHub::new("craft-update-test").get("https://api.github.com/repos/treeforjs/pdfcraft/releases?per_page=5", 4 << 20).unwrap();
    assert!(body.starts_with(b"["), "{}", String::from_utf8_lossy(&body));
    assert!(matches!(GitHub::new("x").get("http://api.github.com/", 10), Err(Error::Invalid(_))));
    // Release downloads redirect to GitHub's object storage; the redirect is followed.
    let mut got = Vec::new();
    GitHub::new("craft-update-test")
        .download("https://github.com/goreleaser/nfpm/releases/download/v2.47.0/checksums.txt", 1 << 20, &mut |chunk| {
            got.extend_from_slice(chunk);
            Ok(())
        })
        .unwrap();
    assert!(String::from_utf8_lossy(&got).contains("nfpm_2.47.0"), "{}", String::from_utf8_lossy(&got));
}
