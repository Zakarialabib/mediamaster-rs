// recv() on tokio::sync::mpsc yields Option<T> (None = channel closed).
use tokio::sync::mpsc;

#[tokio::test]
async fn probe() {
    let (tx, mut rx) = mpsc::channel::<i32>(4);
    let _ = tx.send(42).await;
    let v = rx.recv().await;
    assert_eq!(v, Some(42));
}
