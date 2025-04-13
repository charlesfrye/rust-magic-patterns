use std::{
    f32::consts::PI,
    time::{Duration, Instant},
    fs::File,
    io::{BufWriter, Write},
};

use anyhow::Result;
use futures::FutureExt;
use serde::Serialize;
use tokio::sync::mpsc;

const N: usize = 500;
const K: f32 = 16. * PI / N as f32;

#[derive(Serialize, Debug, Clone)]
struct Sample {
    fut_name: String,
    value: f32,
    start: u128,
    end: u128,
    thread_id: usize,
}

fn sin(i: usize) -> f32 {
    std::thread::sleep(Duration::from_micros(100));
    (K * i as f32).sin()
}

fn sin_heavy(i: usize) -> f32 {
    std::thread::sleep(Duration::from_micros(500));
    (K * i as f32).sin()
}

async fn produce_sin(
    run_start: Instant,
    fut_name: impl ToString,
    tx: mpsc::UnboundedSender<Sample>,
) {
    for i in 1..N {
        let start = run_start.elapsed().as_micros();
        let value = sin(i);
        let end = run_start.elapsed().as_micros();

        let sample = Sample {
            fut_name: fut_name.to_string(),
            value,
            start,
            end,
            thread_id: thread_id::get(),
        };

        tx.send(sample).unwrap();
        tokio::task::yield_now().await;
    }
}

async fn produce_sin_heavy(
    run_start: Instant,
    fut_name: impl ToString,
    tx: mpsc::UnboundedSender<Sample>,
) {
    for i in 1..N {
        let start = run_start.elapsed().as_micros();
        let value = sin_heavy(i);
        let end = run_start.elapsed().as_micros();

        let sample = Sample {
            fut_name: fut_name.to_string(),
            value,
            start,
            end,
            thread_id: thread_id::get(),
        };

        tx.send(sample).unwrap();
        tokio::task::yield_now().await;
    }
}

async fn produce_sin_heavy_blocking(
    run_start: Instant,
    fut_name: impl ToString,
    tx: mpsc::UnboundedSender<Sample>,
) {
    for i in 1..N {
        let start = run_start.elapsed().as_micros();
        let tx = tx.clone();

        let (t_id, value) = tokio::task::spawn_blocking(move || {
            let value = sin_heavy(i);
            let t_id = thread_id::get();

            (t_id, value)
        })
        .await
        .unwrap();

        let end = run_start.elapsed().as_micros();

        let sample = Sample {
            fut_name: fut_name.to_string(),
            value,
            start,
            end,
            thread_id: t_id,
        };

        tx.send(sample).unwrap();

        tokio::task::yield_now().await;
    }
}

async fn two_futures() -> Vec<Sample> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut futs = Vec::new();

    let run_start = Instant::now();

    futs.push(produce_sin(run_start, "fut0", tx.clone()).boxed());
    futs.push(produce_sin(run_start, "fut1", tx.clone()).boxed());

    futures::future::join_all(futs).await;
    drop(tx);

    let mut samples = Vec::new();

    while let Some(next) = rx.recv().await {
        samples.push(next);
    }

    // draw_samples(samples, false, "output/two_futures.png").unwrap();
    samples
}

async fn cpu_intensive() -> Vec<Sample> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut futs = Vec::new();

    let run_start = Instant::now();

    futs.push(produce_sin(run_start, "fut0", tx.clone()).boxed());
    futs.push(produce_sin(run_start, "fut1", tx.clone()).boxed());
    futs.push(produce_sin_heavy(run_start, "high cpu", tx.clone()).boxed());

    futures::future::join_all(futs).await;
    drop(tx);

    let mut samples = Vec::new();

    while let Some(next) = rx.recv().await {
        samples.push(next);
    }

    samples
}

async fn spawn_task() -> Vec<Sample> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut futs = Vec::new();

    let run_start = Instant::now();

    futs.push(produce_sin(run_start, "fut0", tx.clone()).boxed());
    futs.push(produce_sin(run_start, "fut1", tx.clone()).boxed());

    futs.push(
        tokio::spawn(produce_sin_heavy(run_start, "spawned", tx.clone()).boxed())
            .map(|_| ())
            .boxed(),
    );

    futures::future::join_all(futs).await;
    drop(tx);

    let mut samples = Vec::new();

    while let Some(next) = rx.recv().await {
        samples.push(next);
    }

    samples
}

async fn many_spawn_task() -> Vec<Sample> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut futs = Vec::new();

    let run_start = Instant::now();

    futs.push(produce_sin(run_start, "fut0", tx.clone()).boxed());

    for i in 1..7 {
        futs.push(
            tokio::spawn(produce_sin_heavy(run_start, format!("spawned{i}"), tx.clone()).boxed())
                .map(|_| ())
                .boxed(),
        );
    }

    futures::future::join_all(futs).await;
    drop(tx);

    let mut samples = Vec::new();

    while let Some(next) = rx.recv().await {
        samples.push(next);
    }

    samples
}

async fn many_spawn_blocking() -> Vec<Sample> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut futs = Vec::new();

    let run_start = Instant::now();

    futs.push(produce_sin(run_start, "fut0", tx.clone()).boxed());

    for i in 1..7 {
        futs.push(
            produce_sin_heavy_blocking(run_start, format!("spawn_\nblocking{i}"), tx.clone())
                .boxed(),
        );
    }

    futures::future::join_all(futs).await;
    drop(tx);

    let mut samples = Vec::new();

    while let Some(next) = rx.recv().await {
        samples.push(next);
    }

    samples
}

fn write_samples(samples: Vec<Sample>, output_filename: &str) -> Result<()> {
    let file = File::create(output_filename)?;
    let mut writer = BufWriter::new(file);

    for sample in samples {
        let json_line = serde_json::to_string(&sample)?;
        writeln!(writer, "{}", json_line)?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let two_futures_samples = two_futures().await;
    write_samples(two_futures_samples, "resources/two_futures.jsonl")?;

    let spawn_task_samples = spawn_task().await;
    write_samples(spawn_task_samples, "resources/spawn_task.jsonl")?;

    let many_spawn_task_samples = many_spawn_task().await;
    write_samples(many_spawn_task_samples, "resources/many_spawn_task.jsonl")?;

    let cpu_intensive_samples = cpu_intensive().await;
    write_samples(cpu_intensive_samples, "resources/cpu_intensive.jsonl")?;

    let many_spawn_blocking_samples = many_spawn_blocking().await;
    write_samples(many_spawn_blocking_samples, "resources/many_spawn_blocking.jsonl")?;

    Ok(())
}
