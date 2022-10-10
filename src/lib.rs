//! mintex is a *min*imal Mutex.
//!
//! Most of the implementation is lifted from [`std::sync::Mutex`].
//! The reason for this mutex existing is that I'd like a mutex which is
//! quite lightweight and does not perform allocations.

use std::{
    cell::UnsafeCell,
    fmt,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

// 200 microseconds is emprically chosen as a reasonable pause time.
const SLEEP_DURATION: Duration = Duration::from_micros(200);

/// Mutex implementation.
pub struct Mutex<T: ?Sized> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> From<T> for Mutex<T> {
    /// Creates a new mutex in an unlocked state ready for use.
    /// This is equivalent to [`Mutex::new`].
    fn from(t: T) -> Self {
        Mutex::new(t)
    }
}

impl<T: ?Sized + Default> Default for Mutex<T> {
    /// Creates a `Mutex<T>`, with the `Default` value for T.
    fn default() -> Mutex<T> {
        Mutex::new(Default::default())
    }
}

impl<T> Mutex<T> {
    #[inline]
    /// Create a new Mutex which wraps the provided data.
    pub fn new(data: T) -> Self {
        Self {
            lock: Default::default(),
            data: UnsafeCell::new(data),
        }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquire a lock which returns a RAII MutexGuard over the locked data.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        loop {
            match self
                .lock
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(v) => {
                    debug_assert!(!v);
                    unsafe {
                        return MutexGuard::new(self);
                    }
                }
                Err(_e) => {
                    // It is possible that we'll trigger a panic here.
                    // If the thread is already panicking, then we may
                    // hit a variant on:
                    // https://github.com/rust-lang/rust/issues/102398
                    // Not much I can do about that, Even if I check
                    // [`std::thread::panicking()`] that won't help because
                    // the check would be racy and the thread might panic
                    // after the check.
                    // You'll know that you've hit this problem if you
                    // get a panic while panicking error. On MacOS/M1
                    // that's a SIGTRAP, I think it's a SIGILL on Linux.
                    std::thread::park_timeout(SLEEP_DURATION);
                }
            }
        }
    }
    /// Unlock a mutex by dropping the MutexGuard.
    pub fn unlock(guard: MutexGuard<'_, T>) {
        drop(guard);
    }
}

/// RAII Guard over locked data.
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    mutex: &'a Mutex<T>,
}

// It would be nice to mark the MutexGuard as !Sync, but not stable yet.
// impl<T: ?Sized> !Send for MutexGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}

impl<'mutex, T: ?Sized> MutexGuard<'mutex, T> {
    unsafe fn new(mutex: &'mutex Mutex<T>) -> MutexGuard<'mutex, T> {
        MutexGuard { mutex }
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        match self
            .mutex
            .lock
            .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(v) => {
                debug_assert!(v);
            }
            Err(e) => {
                panic!("lock is broken!: {}", e);
            }
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn exercise_mutex_lock() {
        const N: usize = 1000;

        // Spawn a few threads to increment a shared variable (non-atomically), and
        // let the main thread know once all increments are done.

        let (tx, rx) = channel();

        let data: usize = 0;

        let my_lock = Arc::new(Mutex::new(data));

        for _ in 0..N {
            let tx = tx.clone();
            let my_lock = my_lock.clone();
            thread::spawn(move || {
                let mut data = my_lock.lock();
                *data += 1;
                println!("after data: {}", data);
                if *data == N {
                    tx.send(()).unwrap();
                }
            });
        }

        rx.recv().unwrap();
    }
}
