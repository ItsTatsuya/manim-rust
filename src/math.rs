//! Equations typeset by an embedded [Typst](https://typst.app) compiler: no TeX
//! installation, no subprocess, and typical equations compile in milliseconds.
//!
//! [`MathTex`] takes Typst math syntax (`x^2 + y^2 = r^2`, `integral_0^1 f(x) dif x`,
//! `sum_(k=1)^n k = (n(n+1))/2`); [`Typst`] takes any Typst markup.
//! The result is a group of glyph and line paths, so `Write`, `Create` and transforms
//! work as on any other mobject.

#![allow(clippy::new_ret_no_self)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;

use crate::color::Color;
use crate::constants::DEFAULT_FONT_SIZE;
use crate::mobject::VMobject;

/// Typst sizes the page in points; the text is set at this size.
const BASE_PT: f64 = 11.0;
/// Scene units per em at font size 1 (same as `Text`).
const UNITS_PER_FONT_SIZE: f64 = 0.015;
/// `typst_svg` sizes the root in `pt`, which usvg converts to CSS px (96/72);
/// dividing it back out makes one parsed unit one Typst point again.
const PX_PER_PT: f64 = 96.0 / 72.0;

struct Shared {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn shared() -> &'static Shared {
    static SHARED: OnceLock<Shared> = OnceLock::new();
    SHARED.get_or_init(|| {
        let fonts: Vec<Font> = typst_assets::fonts().flat_map(|data| Font::iter(Bytes::new(data))).collect();
        let book = FontBook::from_fonts(&fonts);
        Shared { library: LazyHash::new(Library::default()), book: LazyHash::new(book), fonts }
    })
}

struct MathWorld {
    main: FileId,
    source: Source,
}

impl World for MathWorld {
    fn library(&self) -> &LazyHash<Library> {
        &shared().library
    }
    fn book(&self) -> &LazyHash<FontBook> {
        &shared().book
    }
    fn main(&self) -> FileId {
        self.main
    }
    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            Ok(self.source.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }
    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            Ok(Bytes::from_string(self.source.clone()))
        } else {
            Err(FileError::AccessDenied)
        }
    }
    fn font(&self, index: usize) -> Option<Font> {
        shared().fonts.get(index).cloned()
    }
    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}

fn hex(c: Color) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

/// Longest markup accepted (bytes). Real equations are a few hundred bytes; the cap keeps
/// generated or hostile input from reaching the compiler.
pub const MAX_MARKUP_BYTES: usize = 64 * 1024;

/// A compile still running after this long is abandoned (a hung or runaway document).
const MAX_COMPILE_TIME: Duration = Duration::from_secs(30);

type Interrupt = Rc<dyn Fn() -> bool>;

thread_local! {
    static INTERRUPT: RefCell<Option<Interrupt>> = const { RefCell::new(None) };
}

/// Run `f` so that Typst compiles started on this thread stop waiting (and fail) as soon as
/// `interrupted()` returns true. Scripts and scenes use it to make `--timeout` and cancel
/// reach the compiler; the compile itself cannot be stopped, so it finishes (or not) on a
/// detached worker thread whose result is discarded.
pub fn with_interrupt<R>(interrupted: impl Fn() -> bool + 'static, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Interrupt>);
    impl Drop for Restore {
        fn drop(&mut self) {
            INTERRUPT.with(|i| *i.borrow_mut() = self.0.take());
        }
    }
    let prev = INTERRUPT.with(|i| i.borrow_mut().replace(Rc::new(interrupted)));
    let _restore = Restore(prev);
    f()
}

fn interrupted() -> bool {
    let hook = INTERRUPT.with(|i| i.borrow().clone());
    hook.is_some_and(|h| h())
}

fn compile_svg(doc: String) -> Result<String> {
    let path = VirtualPath::new("main.typ").map_err(|e| anyhow!("{e:?}"))?;
    let main = FileId::new(RootedPath::new(VirtualRoot::Project, path));
    let world = MathWorld { main, source: Source::new(main, doc) };
    let warned = typst::compile::<PagedDocument>(&world);
    // Drop cache entries that have gone unused for a while, so memory stays bounded
    // however many equations a long session compiles (typst-cli does the same).
    typst::comemo::evict(10);
    let doc = warned.output.map_err(|errs| {
        let msgs: Vec<String> = errs.iter().map(|e| e.message.to_string()).collect();
        anyhow!("typst: {}", msgs.join("; "))
    })?;
    let Some(page) = doc.pages().first() else { bail!("typst produced no pages") };
    Ok(typst_svg::svg(page, &typst_svg::SvgOptions::default()))
}

/// Compile Typst markup to SVG (one page, sized to its content).
///
/// The compile runs on a worker thread; the caller waits for it, giving up when
/// [`with_interrupt`]'s check fires or after 30 seconds.
pub fn typst_to_svg(markup: &str, color: Color) -> Result<String> {
    if markup.len() > MAX_MARKUP_BYTES {
        bail!("typst markup is {} bytes, above the {MAX_MARKUP_BYTES}-byte limit", markup.len());
    }
    if interrupted() {
        bail!("typst compile cancelled");
    }
    let doc = format!(
        "#set page(width: auto, height: auto, margin: 0pt, fill: none)
#set text(size: {BASE_PT}pt, fill: rgb(\"{}\"))
{markup}
",
        hex(color)
    );
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("manim-typst".into())
        .stack_size(16 << 20)
        .spawn(move || {
            let _ = tx.send(compile_svg(doc));
        })
        .map_err(|e| anyhow!("cannot start the typst compiler thread: {e}"))?;
    let started = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(r) => return r,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if interrupted() {
                    bail!("typst compile cancelled or out of time");
                }
                if started.elapsed() > MAX_COMPILE_TIME {
                    bail!("typst compile took longer than {}s and was abandoned", MAX_COMPILE_TIME.as_secs());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("the typst compiler crashed"),
        }
    }
}

/// Results kept for repeated equations (a script that rebuilds the same `MathTex` every
/// frame, or a host scrubbing a timeline). Bounded by entries and by total curves.
struct ResultCache {
    /// Least recently used first.
    entries: Vec<(ResultKey, VMobject, usize)>,
    curves: usize,
}

type ResultKey = (String, u64, [u8; 3]);

const MAX_CACHED_RESULTS: usize = 128;
const MAX_CACHED_CURVES: usize = 1 << 20;

static RESULTS: std::sync::Mutex<ResultCache> = std::sync::Mutex::new(ResultCache { entries: Vec::new(), curves: 0 });

impl ResultCache {
    fn get(&mut self, key: &ResultKey) -> Option<VMobject> {
        let i = self.entries.iter().position(|e| &e.0 == key)?;
        let entry = self.entries.remove(i);
        let m = entry.1.clone();
        self.entries.push(entry);
        Some(m)
    }

    fn insert(&mut self, key: ResultKey, m: VMobject) {
        let cost = m.num_curves() + 1;
        if cost > MAX_CACHED_CURVES / 4 {
            return; // one huge result should not push everything else out
        }
        if let Some(i) = self.entries.iter().position(|e| e.0 == key) {
            self.curves -= self.entries.remove(i).2;
        }
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_CACHED_RESULTS || self.curves + cost > MAX_CACHED_CURVES)
        {
            self.curves -= self.entries.remove(0).2;
        }
        self.curves += cost;
        self.entries.push((key, m, cost));
    }
}

/// Typeset markup at `font_size` (Manim units: 48 = default text size).
///
/// Only successful results are cached; a miss goes through [`typst_to_svg`] with its size
/// limit, cancel check and timeout, and a cancelled, timed out or failed compile is an
/// error that is never stored.
pub fn typeset(markup: &str, font_size: f64, color: Color) -> Result<VMobject> {
    if markup.len() > MAX_MARKUP_BYTES {
        return typeset_uncached(markup, font_size, color); // the size error, without copying
    }
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    // The colour enters the document through `hex`, so equal hex means an equal result.
    let key: ResultKey = (markup.to_string(), font_size.to_bits(), [b(color.r), b(color.g), b(color.b)]);
    if !interrupted() {
        if let Some(m) = RESULTS.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(m);
        }
    }
    let m = typeset_uncached(markup, font_size, color)?;
    RESULTS.lock().unwrap_or_else(|e| e.into_inner()).insert(key, m.clone());
    Ok(m)
}

fn typeset_uncached(markup: &str, font_size: f64, color: Color) -> Result<VMobject> {
    let svg = typst_to_svg(markup, color)?;
    // One Typst point → scene units, so BASE_PT points = one em at `font_size`.
    let scale = font_size * UNITS_PER_FONT_SIZE / (BASE_PT * PX_PER_PT);
    let mut m = crate::svg::parse(svg.as_bytes(), scale)?.center();
    m.for_each_mut(&mut |c| c.meta.nonzero = true);
    Ok(m)
}

/// A display equation in Typst math syntax (Manim's `MathTex`).
pub struct MathTex;
impl MathTex {
    /// Typeset `math` (Typst math syntax) at the default font size and the current default text color. Errors on Typst syntax errors, markup over 64 KiB, or a compile over 30 s.
    pub fn new(math: &str) -> Result<VMobject> {
        Self::styled(math, DEFAULT_FONT_SIZE, crate::defaults::get().text_color)
    }

    /// Like [`MathTex::new`] with an explicit font size (points) and color.
    pub fn styled(math: &str, font_size: f64, color: Color) -> Result<VMobject> {
        Ok(typeset(&format!("$ {math} $"), font_size, color)?.with_kind("Math").with_label(math))
    }
}

/// Inline Typst markup (text with `$math$`, `*bold*`, lists...) (Manim's `Tex`).
pub struct Typst;
impl Typst {
    /// Typeset Typst `markup` at the default font size and the current default text color. Errors as [`MathTex::new`] does.
    pub fn new(markup: &str) -> Result<VMobject> {
        Self::styled(markup, DEFAULT_FONT_SIZE, crate::defaults::get().text_color)
    }

    /// Like [`Typst::new`] with an explicit font size (points) and color.
    pub fn styled(markup: &str, font_size: f64, color: Color) -> Result<VMobject> {
        Ok(typeset(markup, font_size, color)?.with_kind("Math").with_label(markup))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typesets_an_equation() {
        let m = MathTex::new("x^2 + y^2 = r^2").unwrap();
        assert!(m.children.len() >= 5, "glyphs: {}", m.children.len());
        assert!(m.width() > m.height());
        assert!(m.height() > 0.2 && m.height() < 2.0, "height {}", m.height());
        assert!(MathTex::new("undefinedthing(x)").is_err());
    }

    #[test]
    fn repeated_equations_are_served_from_the_cache() {
        let (white, red) = (crate::color::WHITE, crate::color::RED);
        let a = typeset("$ a^2 + b^2 $", 48.0, white).unwrap();
        let cached = || RESULTS.lock().unwrap().entries.iter().any(|e| e.0 .0 == "$ a^2 + b^2 $");
        assert!(cached());
        let b = typeset("$ a^2 + b^2 $", 48.0, white).unwrap();
        assert_eq!((a.width(), a.num_curves()), (b.width(), b.num_curves()));
        // Size and colour are part of the key.
        let big = typeset("$ a^2 + b^2 $", 96.0, white).unwrap();
        assert!((big.width() - 2.0 * a.width()).abs() < 1e-6);
        let colored = typeset("$ a^2 + b^2 $", 48.0, red).unwrap();
        assert_eq!(colored.family()[1].fill_color, red);
        assert_eq!(a.family()[1].fill_color, white);
        // A clone out of the cache is independent: editing it does not touch the next hit.
        let moved = typeset("$ a^2 + b^2 $", 48.0, white).unwrap().shift(crate::constants::RIGHT * 5.0);
        assert!(moved.get_center().x > 4.0);
        assert!(typeset("$ a^2 + b^2 $", 48.0, white).unwrap().get_center().x.abs() < 1e-6);
    }

    #[test]
    fn failures_and_cancelled_compiles_are_not_cached() {
        let bad = "$ cached_failure_thing(x) $";
        assert!(typeset(bad, 48.0, crate::color::WHITE).is_err());
        assert!(!RESULTS.lock().unwrap().entries.iter().any(|e| e.0 .0 == bad));
        // A cancelled miss is an error and leaves nothing behind.
        let fresh = "$ cancelled_before_it_started + 1 $";
        let r = with_interrupt(|| true, || typeset(fresh, 48.0, crate::color::WHITE));
        assert!(r.is_err());
        assert!(!RESULTS.lock().unwrap().entries.iter().any(|e| e.0 .0 == fresh));
        // A cancelled caller gets the error even for an equation that is cached.
        let _ = typeset("$ q $", 48.0, crate::color::WHITE).unwrap();
        assert!(with_interrupt(|| true, || typeset("$ q $", 48.0, crate::color::WHITE)).is_err());
    }

    #[test]
    fn cache_is_bounded() {
        let mut c = ResultCache { entries: Vec::new(), curves: 0 };
        let m = crate::mobject::VMobject::group(vec![]);
        for i in 0..MAX_CACHED_RESULTS * 2 {
            c.insert((format!("k{i}"), 0, [0; 3]), m.clone());
        }
        assert_eq!(c.entries.len(), MAX_CACHED_RESULTS);
        assert!(c.get(&("k0".into(), 0, [0; 3])).is_none(), "oldest entries are evicted");
        assert!(c.get(&(format!("k{}", MAX_CACHED_RESULTS * 2 - 1), 0, [0; 3])).is_some());
    }

    #[test]
    fn long_markup_is_rejected() {
        let big = "x".repeat(MAX_MARKUP_BYTES + 1);
        let e = typst_to_svg(&big, crate::color::WHITE).unwrap_err();
        assert!(e.to_string().contains("limit"), "{e}");
    }

    #[test]
    fn an_interrupt_stops_a_runaway_compile() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        // Several seconds of work if left to finish.
        let src = "#let n = 0
#for i in range(6000) { for j in range(6000) { n += 1 } }
#n";
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            flag.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let r = with_interrupt(move || stop.load(Ordering::Relaxed), || typst_to_svg(src, crate::color::WHITE));
        setter.join().unwrap();
        assert!(r.is_err());
        assert!(t.elapsed() < Duration::from_secs(2), "took {:?}", t.elapsed());
    }

    #[test]
    fn digit_height_matches_font_size() {
        // A New Computer Modern digit is ~0.665 em, and an em is 0.015 units per font size:
        // at 48 that is ~0.48 units (it was 0.639 when usvg's pt→px factor leaked through).
        let h = MathTex::styled("2", 48.0, crate::color::WHITE).unwrap().height();
        assert!((h - 0.4795).abs() < 0.005, "digit height {h}");
        let h96 = MathTex::styled("2", 96.0, crate::color::WHITE).unwrap().height();
        assert!((h96 - 2.0 * h).abs() < 1e-6, "height must scale linearly: {h} vs {h96}");
    }
}
