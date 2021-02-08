use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;
use tokio::time::sleep;
use tracing::{debug, span, Level};
use tracing_subscriber;

async fn sleep_and_return() -> u32 {
    sleep(Duration::from_secs(10)).await;
    26
}

fn call_async_fn() -> Pin<Box<dyn Future<Output = u32> + Send>> {
    Box::pin(sleep_and_return())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let span = span!(Level::DEBUG, "main");
    let _enter = span.enter();

    //let once: OnceCell<u32> = OnceCell::new();
    let once = Arc::new(OnceCell::new());
    let once_clone1 = once.clone();
    let once_clone2 = once.clone();
    let handle1 = tokio::spawn(async move {
        let result1 = once_clone1.get_or_init(call_async_fn).await;
    });
    let handle2 = tokio::spawn(async move {
        let result2 = once_clone2.get_or_init(call_async_fn).await;
    });

    handle1.await;
    handle2.await;
    let result = once.get();
    if let Some(ref v) = result {
        println!("result: {:?}", v);
    }
}
