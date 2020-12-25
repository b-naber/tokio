use crate::sync::batch_semaphore::Semaphore;
use std::fmt;
use std::marker;
use std::mem;
use std::ops;

/// RAII structure used to release the exclusive write access of a lock when
/// dropped.
///
/// This structure is created by the [`write`] and method
/// on [`RwLock`].
///
/// [`write`]: method@crate::sync::RwLock::write
/// [`RwLock`]: struct@crate::sync::RwLock
pub struct RwLockMappedWriteGuard<'a, T: ?Sized> {
    pub(super) s: &'a Semaphore,
    pub(super) data: *mut T,
    pub(super) marker: marker::PhantomData<&'a mut T>,
}

impl<'a, T: ?Sized> RwLockMappedWriteGuard<'a, T> {
    /// Make a new `RwLockMappedWriteGuard` for a component of the locked data.
    ///
    /// This operation cannot fail as the `RwLockMappedWriteGuard` passed in already
    /// locked the data.
    ///
    /// This is an associated function that needs to be used as
    /// `RwLockWriteGuard::map(..)`. A method would interfere with methods of
    /// the same name on the contents of the locked data.
    ///
    /// This is an asynchronous version of [`RwLockWriteGuard::map`] from the
    /// [`parking_lot` crate].
    ///
    /// [`RwLockWriteGuard::map`]: https://docs.rs/lock_api/latest/lock_api/struct.RwLockWriteGuard.html#method.map
    /// [`parking_lot` crate]: https://crates.io/crates/parking_lot
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::sync::{RwLock, RwLockWriteGuard};
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// struct Foo(u32);
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let lock = RwLock::new(Foo(1));
    ///
    /// {
    ///     let mut mapped = RwLockWriteGuard::map(lock.write().await, |f| &mut f.0);
    ///     *mapped = 2;
    /// }
    ///
    /// assert_eq!(Foo(2), *lock.read().await);
    /// # }
    /// ```
    #[inline]
    pub fn map<F, U: ?Sized>(mut this: Self, f: F) -> RwLockMappedWriteGuard<'a, U>
    where
        F: FnOnce(&mut T) -> &mut U,
    {
        let data = f(&mut *this) as *mut U;
        let s = this.s;
        // NB: Forget to avoid drop impl from being called.
        mem::forget(this);
        RwLockMappedWriteGuard {
            s,
            data,
            marker: marker::PhantomData,
        }
    }

    /// Attempts to make  a new [`RwLockMappedWriteGuard`] for a component of
    /// the locked data. The original guard is returned if the closure returns
    /// `None`.
    ///
    /// This operation cannot fail as the `RwLockWriteGuard` passed in already
    /// locked the data.
    ///
    /// This is an associated function that needs to be
    /// used as `RwLockWriteGuard::try_map(...)`. A method would interfere with
    /// methods of the same name on the contents of the locked data.
    ///
    /// This is an asynchronous version of [`RwLockWriteGuard::try_map`] from
    /// the [`parking_lot` crate].
    ///
    /// [`RwLockWriteGuard::try_map`]: https://docs.rs/lock_api/latest/lock_api/struct.RwLockWriteGuard.html#method.try_map
    /// [`parking_lot` crate]: https://crates.io/crates/parking_lot
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::sync::{RwLock, RwLockWriteGuard};
    ///
    /// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// struct Foo(u32);
    ///
    /// # #[tokio::main]
    /// # async fn main() {
    /// let lock = RwLock::new(Foo(1));
    ///
    /// {
    ///     let guard = lock.write().await;
    ///     let mut guard = RwLockWriteGuard::try_map(guard, |f| Some(&mut f.0)).expect("should not fail");
    ///     *guard = 2;
    /// }
    ///
    /// assert_eq!(Foo(2), *lock.read().await);
    /// # }
    /// ```
    #[inline]
    pub fn try_map<F, U: ?Sized>(
        mut this: Self,
        f: F,
    ) -> Result<RwLockMappedWriteGuard<'a, U>, Self>
    where
        F: FnOnce(&mut T) -> Option<&mut U>,
    {
        let data = match f(&mut *this) {
            Some(data) => data as *mut U,
            None => return Err(this),
        };
        let s = this.s;
        // NB: Forget to avoid drop impl from being called.
        mem::forget(this);
        Ok(RwLockMappedWriteGuard {
            s,
            data,
            marker: marker::PhantomData,
        })
    }
}

impl<T: ?Sized> ops::Deref for RwLockMappedWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.data }
    }
}

impl<T: ?Sized> ops::DerefMut for RwLockMappedWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.data }
    }
}

impl<'a, T: ?Sized> fmt::Debug for RwLockMappedWriteGuard<'a, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<'a, T: ?Sized> fmt::Display for RwLockMappedWriteGuard<'a, T>
where
    T: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<'a, T: ?Sized> Drop for RwLockMappedWriteGuard<'a, T> {
    fn drop(&mut self) {
        self.s.release(super::MAX_READS);
    }
}
