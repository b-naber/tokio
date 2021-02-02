#![cfg_attr(not(feature = "sync"), allow(dead_code, unreachable_pub))]

use crate::loom::cell::UnsafeCell;
use std::future::Future;
use std::marker::Unpin;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

pub struct OnceCell<T> {
    inner: Arc<Inner<T>>,
}

struct Inner<T> {
    guard: Mutex<()>,
    state: State,
    value: UnsafeCell<Option<T>>,
    waker: Mutex<Option<Waker>>,
}

pub struct OnceCellResult<T> {
    handle: Arc<Inner<T>>,
}

#[derive(Debug)]
struct State(AtomicUsize);

const UNITIALIZED: usize = 0b00001;
const INITIALIZED: usize = 0b00010;
const INITIALIZING: usize = 0b00100;

unsafe impl<T: Sync + Send> Sync for OnceCell<T> {}
unsafe impl<T: Send + Sync> Send for OnceCell<T> {}
unsafe impl<T: Sync + Send> Sync for Inner<T> {}
unsafe impl<T: Send + Sync> Send for Inner<T> {}
unsafe impl Sync for State {}
unsafe impl Send for State {}
unsafe impl<T: Sync + Send> Sync for OnceCellResult<T> {}
unsafe impl<T: Send + Sync> Send for OnceCellResult<T> {}

// I think this is fine, because OnceCellResult is non-self-referential
// if we want to assure that T is never moved, we would have to
// call Pin on T itself.
impl<T: Send + Sync> Unpin for OnceCellResult<T> {}

impl<T> Inner<T> {
    pub(crate) fn new() -> Inner<T> {
        Inner {
            guard: Mutex::new(()),
            state: State::new(),
            value: UnsafeCell::new(None),
            waker: Mutex::new(None),
        }
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.state.is_initialized()
    }

    pub(crate) fn is_initializing(&self) -> bool {
        self.state.is_initializing()
    }

    pub(crate) fn is_uninitialized(&self) -> bool {
        self.state.is_uninitialized()
    }
}

impl<T> OnceCell<T> {
    pub fn new() -> OnceCell<T> {
        OnceCell {
            inner: Arc::new(Inner::new()),
        }
    }

    pub fn is_initialized(&self) -> bool {
        self.inner.state.is_initialized()
    }

    fn is_initializing(&self) -> bool {
        self.inner.state.is_initializing()
    }

    pub fn is_uninitialized(&self) -> bool {
        self.inner.state.is_uninitialized()
    }

    pub fn get(&self) -> Option<&T> {
        if self.inner.state.is_initialized() {
            Some(unsafe { self.get_unchecked_ref() })
        } else {
            None
        }
    }

    unsafe fn get_unchecked_ref(&self) -> &T {
        debug_assert!(self.inner.state.is_initialized());
        self.inner.value.with(|ptr| {
            if let Some(ref v) = *ptr {
                v
            } else {
                panic!();
            }
        })
    }

    unsafe fn get_unchecked_mut_ref(&self) -> &mut T {
        debug_assert!(self.inner.state.is_initialized());
        self.inner.value.with_mut(|ptr| {
            if let Some(ref mut v) = *ptr {
                v
            } else {
                panic!();
            }
        })
    }
}

impl<T: Send + Sync> OnceCell<T> {
    pub async fn get_or_init<F>(&self, f: F) -> Result<&T, OnceCellErrors>
    where
        F: Fn() -> Pin<Box<dyn Future<Output = T> + Send + 'static>>,
    {
        if self.is_initialized() {
            Ok(unsafe { self.get_unchecked_ref() })
        } else if self.is_initializing() {
            OnceCellResult::new(&self).await;
            Ok(unsafe { self.get_unchecked_ref() })
        } else if self.is_uninitialized() {
            if self.is_initializing() {
                OnceCellResult::new(&self).await;
                return Ok(unsafe { self.get_unchecked_ref() });
            }

            self.inner.state.set_to_initializing();
            let result = f().await;

            let _guard = self.inner.guard.lock().unwrap();
            self.inner.value.with_mut(|ptr| {
                let v_ref: &mut Option<T> = unsafe { &mut *ptr };
                *v_ref = Some(result);
            });
            self.inner.state.set_to_initialized();

            // Wake tasks waiting for intitialization to finish
            if let Some(ref waker) = *self.inner.waker.lock().unwrap() {
                waker.wake_by_ref();
            }

            Ok(unsafe { self.get_unchecked_ref() })
        } else {
            // currently here only for debugging
            Err(OnceCellErrors::DebugState)
        }
    }

    pub fn set(&self, value: T) -> Result<(), OnceCellErrors> {
        if self.inner.state.is_initialized() || self.inner.state.is_initializing() {
            Err(OnceCellErrors::InitializedOrInitializing)
        } else {
            self.inner.state.set_to_initializing();

            let _guard = self.inner.guard.lock().unwrap();
            self.inner.value.with_mut(|ptr| {
                let v_ref: &mut Option<T> = unsafe { &mut *ptr };
                *v_ref = Some(value);
            });
            self.inner.state.set_to_initialized();

            Ok(())
        }
    }
}

impl<T> OnceCellResult<T> {
    pub fn new(cell: &OnceCell<T>) -> Self {
        OnceCellResult {
            handle: cell.inner.clone(),
        }
    }
}

// Errors
#[derive(Debug)]
pub enum OnceCellErrors {
    PollWhenUninit,
    InitializedOrInitializing,
    DebugState,
}

impl<T: Send + Sync> Future for OnceCellResult<T> {
    type Output = Result<(), OnceCellErrors>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.handle.is_initialized() {
            Poll::Ready(Ok(()))
        } else if self.handle.is_initializing() {
            *self.handle.waker.lock().unwrap() = Some(cx.waker().clone());
            //*waker = Some(cx.waker().clone());

            Poll::Pending
        } else if self.handle.is_uninitialized() {
            // We never give out a OnceCellResult when state is unitialized
            Poll::Ready(Err(OnceCellErrors::DebugState))
        } else {
            panic!();
        }
    }
}

impl<T> Default for OnceCell<T> {
    fn default() -> OnceCell<T> {
        OnceCell::new()
    }
}

impl State {
    const fn new() -> Self {
        State(AtomicUsize::new(UNITIALIZED))
    }

    fn get(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }

    fn is_initialized(&self) -> bool {
        self.0.load(Ordering::Acquire) & INITIALIZED == INITIALIZED
    }

    fn is_initializing(&self) -> bool {
        self.0.load(Ordering::Acquire) & INITIALIZING == INITIALIZING
    }

    fn is_uninitialized(&self) -> bool {
        self.0.load(Ordering::Acquire) & UNITIALIZED == UNITIALIZED
    }

    fn set_to_initialized(&self) {
        self.0.store(INITIALIZED, Ordering::Release)
    }

    fn set_to_initializing(&self) {
        self.0.store(INITIALIZING, Ordering::Release)
    }
}
