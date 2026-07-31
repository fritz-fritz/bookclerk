//! Drives the real [`MediaPool`] through the real worker binary.
//!
//! The isolation tests spawn the worker directly. This one goes through the
//! path production uses — `bookclerk-media`'s public async API, the pool's
//! permit accounting, process spawn, and reply parsing — so a regression
//! anywhere in that chain shows up here.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bookclerk_media::{
    package_m4b_from_pcm, Confinement, MediaJob, MediaPool, MediaPoolConfig, PackageM4bRequest,
};

const WORKER: &str = env!("CARGO_BIN_EXE_bookclerk-media-worker");

fn confined_pool(workers: usize) -> MediaPool {
    MediaPool::new(MediaPoolConfig {
        workers,
        confinement: Confinement::Required,
        worker_bin: Some(PathBuf::from(WORKER)),
    })
}

fn make_audiobook(path: &Path, seconds: usize) {
    let sample_rate = 44_100usize;
    let pcm: Vec<i16> = (0..sample_rate * seconds)
        .map(|n| {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let value = ((n as f32 / 40.0).sin() * 2_000.0) as i16;
            value
        })
        .collect();
    package_m4b_from_pcm(
        &pcm,
        u32::try_from(sample_rate).expect("sample rate fits u32"),
        1,
        path,
        &[("One".to_string(), 0)],
    )
    .expect("build fixture audiobook");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pool_runs_a_real_encode_in_a_confined_worker() {
    if !bookclerk_sandbox::capabilities().filesystem {
        eprintln!("skipping: no filesystem confinement on this host");
        return;
    }

    let cache = tempfile::tempdir().expect("tempdir");
    let out = tempfile::tempdir().expect("tempdir");
    let input = cache.path().join("book.m4b");
    make_audiobook(&input, 2);

    let pool = confined_pool(2);
    assert!(
        pool.is_isolated(),
        "pool should have found the worker binary"
    );

    let output = out.path().join("book.mp3");
    let result = pool
        .run(MediaJob::EncodeMp3 {
            input: input.clone(),
            output: output.clone(),
            lame: Box::default(),
            max_sample_rate: None,
        })
        .await
        .expect("encode through the pool");

    assert_eq!(result.output(), Some(output.as_path()));
    let encoded = std::fs::metadata(&output).expect("encoded file");
    assert!(
        encoded.len() > 1_000,
        "encoded MP3 is implausibly small: {} bytes",
        encoded.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pool_runs_jobs_concurrently_up_to_its_capacity() {
    if !bookclerk_sandbox::capabilities().filesystem {
        eprintln!("skipping: no filesystem confinement on this host");
        return;
    }

    let cache = tempfile::tempdir().expect("tempdir");
    let out = tempfile::tempdir().expect("tempdir");
    let input = cache.path().join("book.m4b");
    make_audiobook(&input, 4);

    let pool = Arc::new(confined_pool(3));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for n in 0..6 {
        let pool = Arc::clone(&pool);
        let in_flight = Arc::clone(&in_flight);
        let peak = Arc::clone(&peak);
        let input = input.clone();
        let output = out.path().join(format!("book-{n}.mp3"));
        handles.push(tokio::spawn(async move {
            let running = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(running, Ordering::SeqCst);
            let result = pool
                .run(MediaJob::EncodeMp3 {
                    input,
                    output,
                    lame: Box::default(),
                    max_sample_rate: None,
                })
                .await;
            in_flight.fetch_sub(1, Ordering::SeqCst);
            result
        }));
    }

    for handle in handles {
        handle
            .await
            .expect("task joined")
            .expect("job succeeded through the pool");
    }

    // Every job wrote its own file, so none of them clobbered another.
    for n in 0..6 {
        let path = out.path().join(format!("book-{n}.mp3"));
        assert!(path.exists(), "missing output for job {n}");
    }
    assert!(
        peak.load(Ordering::SeqCst) > 1,
        "jobs never overlapped; the pool is not running work concurrently"
    );
}

#[tokio::test]
async fn pool_surfaces_a_job_failure_without_killing_the_caller() {
    let cache = tempfile::tempdir().expect("tempdir");
    let out = tempfile::tempdir().expect("tempdir");
    let pool = confined_pool(1);

    let err = pool
        .run(MediaJob::PackageM4b {
            request: Box::new(PackageM4bRequest {
                // Deliberately not audio, so the codec rejects it.
                parts: vec![cache.path().join("not-audio.mp3")],
                output: out.path().join("book.m4b"),
                chapter_titles: vec![],
            }),
        })
        .await
        .expect_err("packaging garbage should fail");
    assert!(
        err.to_string().contains("not-audio"),
        "error should name the offending part: {err}"
    );

    // The pool is still usable afterwards: a crashed or failing job must not
    // poison the permit accounting.
    let input = cache.path().join("book.m4b");
    make_audiobook(&input, 1);
    let output = out.path().join("ok.mp3");
    pool.run(MediaJob::EncodeMp3 {
        input,
        output: output.clone(),
        lame: Box::default(),
        max_sample_rate: None,
    })
    .await
    .expect("pool still works after a failed job");
    assert!(output.exists());
}
