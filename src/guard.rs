//! Panic containment for code that runs on behalf of a host application.
//!
//! The library should not panic, but scripts and animations reach a lot of code; a stray
//! panic must become an error value, and must not print to stderr either (a host reading
//! `--progress json` on stderr would see garbage). The panic hook is process-wide, so one
//! wrapper is installed once; it stays silent only on threads inside [`catch`] and
//! otherwise calls the hook that was there before.

use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Once;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    static WHERE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if DEPTH.with(Cell::get) > 0 {
                let at = info.location().map(|l| format!("{}:{}", l.file(), l.line()));
                WHERE.with(|w| *w.borrow_mut() = at);
            } else {
                previous(info);
            }
        }));
    });
}

/// Run `f`; a panic inside it comes back as `Err(message)` and prints nothing.
pub(crate) fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    install();
    DEPTH.with(|d| d.set(d.get() + 1));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    DEPTH.with(|d| d.set(d.get() - 1));
    result.map_err(|p| {
        let msg = p
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        match WHERE.with(|w| w.borrow_mut().take()) {
            Some(at) => format!("{msg} (at {at})"),
            None => msg,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_becomes_an_error() {
        let e = catch(|| -> u8 { panic!("boom {}", 7) }).unwrap_err();
        assert!(e.starts_with("boom 7"), "{e}");
        assert_eq!(catch(|| 5), Ok(5));
    }
}
